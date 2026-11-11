// Signed method packs as a person uses them: a publisher makes a key and signs a pack with `kaku pack`, the owner pins the key in
// Instructions, imports the pack, and sees every method arrive unapproved; an unpinned pack, a changed pack and a name clash are
// refused in words. Needs `cargo build -p kk-cli`.
//
//   node tests/e2e/packs_web.mjs     screenshots in target/packs-web/

import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/packs-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
const kaku = join(repo, "target/debug/kaku");
const run = (...args) => { const r = spawnSync(kaku, args, { encoding: "utf8" }); if (r.status !== 0) throw new Error(args.join(" ") + ": " + r.stderr); return r.stdout; };
const makePack = (tag, keyFile, name, version, methods) => {
  const draft = { format: "kakurega-pack", version: 1, name, pack_version: version, not_after: 4000000000, methods: methods.map((m) => ({ name: m, description: "How to read a contract", triggers: ["contract"], avoid: [], body: "Quote clause numbers." })) };
  writeFileSync(join(work, `${tag}.json`), JSON.stringify(draft));
  run("pack", "sign", "--key", keyFile, "--pack", join(work, `${tag}.json`), "--out", join(work, `${tag}.kkp`));
  return readFileSync(join(work, `${tag}.kkp`), "utf8");
};
const keyOut = run("pack", "keygen", "--out", join(work, "pub.key"));
const publicKey = /public key \(give this to installations\): (\S+)/.exec(keyOut)[1];
run("pack", "keygen", "--out", join(work, "other.key"));
const good = makePack("good", join(work, "pub.key"), "legal-kit", "1.0.0", ["contract-check", "clause-notes"]);
const stranger = makePack("stranger", join(work, "other.key"), "stranger-kit", "1.0.0", ["other-method"]);

const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(kaku, ["serve", "--data-dir", join(work, "data"), "--root", root, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
process.on("exit", () => { try { server.kill(); } catch {} });
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1100 } })).newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error" && !/status of (422|409)/.test(m.text())) problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { if (!cond) { console.log("FAIL", what); process.exitCode = 1; } else console.log("PASS", what); };
const origin = new URL(link).origin;
const importPack = async (text) => {
  await page.locator("#pack-text").fill(text);
  await page.getByRole("button", { name: "Import pack" }).click();
};

await page.goto(link);
await page.waitForSelector("nav");
await page.goto(origin + "/#/skills");
await page.getByRole("heading", { name: "Method packs" }).waitFor();
check(await page.getByText("No publisher is pinned yet, so no pack can be imported.").isVisible(), "a new install trusts no publisher and says so");
await importPack(good);
await page.getByRole("alert").filter({ hasText: /not signed by a publisher you trust/ }).waitFor();
check(true, "a pack from a publisher that is not pinned is refused in words");
await page.locator("#pack-key").fill(publicKey);
await page.locator("#pack-label").fill("Legal team");
await page.getByRole("button", { name: "Trust this publisher" }).click();
await page.getByText("Publisher pinned.").waitFor();
check(await page.getByText("Legal team").first().isVisible(), "the pinned publisher is listed");
await importPack(stranger);
await page.getByRole("alert").filter({ hasText: /not signed by a publisher you trust/ }).waitFor();
check(true, "a pack signed by another key is still refused");
await importPack(good.replace("A", "B"));
await page.getByRole("alert").filter({ hasText: /not signed by a publisher you trust|changed after signing/ }).waitFor();
check(true, "a pack changed after signing is refused");
await importPack(good);
await page.getByText(/Imported 2 methods from legal-kit 1.0.0/).waitFor();
await shot("1-imported");
check((await page.getByText("Waiting for your approval: not in use").count()) >= 2, "both imported methods wait for approval");
await importPack(good);
await page.getByRole("alert").filter({ hasText: /already exists; nothing was imported/ }).waitFor();
check(true, "importing the same pack again is refused and replaces nothing");
await page.getByRole("button", { name: "Stop trusting" }).click();
await page.getByText("Publisher removed.").waitFor();
check(await page.getByText("No publisher is pinned yet, so no pack can be imported.").isVisible(), "removing the publisher stops trusting it");
check(problems.length === 0, "no console errors: " + problems.join("; "));
await browser.close();
server.kill();
process.exit(process.exitCode ?? 0);
