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
const makePack = (tag, keyFile, name, version, methods, servers) => {
  const draft = { format: "kakurega-pack", version: 1, name, pack_version: version, not_after: 4000000000, methods: methods.map((m) => ({ name: m, description: "How to read a contract", triggers: ["contract"], avoid: [], body: "Quote clause numbers." })), ...(servers ? { servers } : {}) };
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
// packs sit in a fold at the end of the methods; it is opened to work with them
check(!(await page.locator("#packs-fold").evaluate((d) => d.open)), "the packs section is folded until someone wants it");
await page.locator("#packs-fold > summary").click();
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
// ---- a pack that suggests an outside program: shown, never added; it fills the form when asked, and can be put away ----
{
  const tools = makePack("tools", join(work, "pub.key"), "scan-kit", "1.0.0", ["scan-notes"], [{ name: "semgrep", program: "/usr/local/bin/semgrep-mcp", args: ["--stdio"], env: ["SEMGREP_APP_TOKEN"], why: "Pattern rules for reviews" }]);
  await importPack(tools);
  await page.getByText(/Imported 1 method from scan-kit 1.0.0. Read it and approve it if you want it. It also suggests 1 outside program \(semgrep\); nothing was added/).waitFor();
  const suggested = page.locator("section[aria-labelledby='h-mcp-suggested']");
  await suggested.waitFor();
  const card = await suggested.innerText();
  check(/semgrep/.test(card) && /from the pack scan-kit/.test(card) && card.includes("/usr/local/bin/semgrep-mcp") && (await suggested.locator("ol.args li").allInnerTexts()).join("|") === "--stdio" && /SEMGREP_APP_TOKEN/.test(card) && /Pattern rules for reviews/.test(card), "the suggested program is shown exactly: its path, its arguments, the variables it needs and why", card);
  check(await page.getByText("No outside programs yet.").isVisible(), "nothing was added by importing the pack");
  await suggested.getByRole("button", { name: "Fill the form with it" }).click();
  check(await page.locator("#mcp-name").inputValue() === "semgrep" && await page.locator("#mcp-program").inputValue() === "/usr/local/bin/semgrep-mcp" && await page.locator("#mcp-args").inputValue() === "--stdio" && await page.locator("#mcp-env").inputValue() === "SEMGREP_APP_TOKEN=", "the form is filled with it and the value is left for you");
  await suggested.scrollIntoViewIfNeeded();
  await shot("1b-suggested");
  await suggested.getByRole("button", { name: /Dismiss/ }).click();
  await page.getByRole("heading", { name: "Suggested by packs you imported" }).waitFor({ state: "detached" });
  check(true, "a dismissed suggestion is put away");
}
// ---- the publisher's signed catalog ----
{
  const shelf = join(work, "shelf");
  mkdirSync(shelf, { recursive: true });
  writeFileSync(join(shelf, "legal-kit-1.0.0.kkp"), good);
  const notes = makePack("notes", join(work, "pub.key"), "notes-kit", "2.1.0", ["meeting-minutes"]);
  writeFileSync(join(shelf, "notes-kit-2.1.0.kkp"), notes);
  run("pack", "catalog", "--key", join(work, "pub.key"), "--dir", shelf, "--out", join(work, "catalog.kkc"));
  const checked = run("pack", "check-catalog", "--catalog", join(work, "catalog.kkc"), "--trusted", publicKey, "--dir", shelf);
  check(/catalog of 2 packs/.test(checked) && (checked.match(/file matches/g) ?? []).length === 2, "a publisher makes a signed catalog of a folder of packs, and anyone with the key can check it against the files");
  writeFileSync(join(shelf, "notes-kit-2.1.0.kkp"), notes + " ");
  const wrong = spawnSync(kaku, ["pack", "check-catalog", "--catalog", join(work, "catalog.kkc"), "--trusted", publicKey, "--dir", shelf], { encoding: "utf8" });
  check(wrong.status !== 0 && /FILE DOES NOT MATCH/.test(wrong.stdout), "a file changed after the catalog was made is called out");
  writeFileSync(join(shelf, "notes-kit-2.1.0.kkp"), notes);

  await page.locator("#pack-catalog").setInputFiles(join(work, "catalog.kkc"));
  const list = page.locator("#pack-catalog-list");
  await list.waitFor();
  const rows = await list.locator("li").evaluateAll((els) => els.map((e) => e.innerText.replace(/\s+/g, " ")));
  check(rows.length === 2 && /legal-kit 1\.0\.0 Added/.test(rows[0]) && /notes-kit 2\.1\.0 Not added/.test(rows[1]) && /Signed by Legal team/.test(await list.innerText()), "opening the catalog lists the publisher's packs and says which one is already added", rows.join(" | "));
  check((await page.locator("#pack-cat-legal-kit").count()) === 0 && (await page.locator("#pack-cat-notes-kit").count()) === 1, "only a pack that is not added offers to be added");
  // a different file than the one listed: signed by the same publisher, but not these bytes
  const other = makePack("notes-other", join(work, "pub.key"), "notes-kit", "2.1.1", ["meeting-minutes"]);
  await page.locator("#pack-cat-notes-kit").setInputFiles({ name: "notes-kit-2.1.0.kkp", mimeType: "application/octet-stream", buffer: Buffer.from(other) });
  await page.getByRole("alert").filter({ hasText: /not one the catalog lists/ }).waitFor();
  check((await page.getByText("meeting-minutes", { exact: true }).count()) === 0, "a file that is not the one the catalog lists is refused, even from the same publisher");
  await page.locator("#pack-cat-notes-kit").setInputFiles(join(shelf, "notes-kit-2.1.0.kkp"));
  await page.getByText(/Imported 1 methods? from notes-kit 2.1.0/).waitFor();
  await list.locator("li").nth(1).getByText("Added", { exact: true }).waitFor();
  const after = await list.locator("li").evaluateAll((els) => els.map((e) => e.innerText.replace(/\s+/g, " ")));
  check(/notes-kit 2\.1\.0 Added/.test(after[1]), "the exact file is added, and the catalog then says so", after.join(" | "));
  await shot("2-catalog");
}
await page.getByRole("button", { name: "Stop trusting" }).click();
await page.getByText("Publisher removed.").waitFor();
check(await page.getByText("No publisher is pinned yet, so no pack can be imported.").isVisible(), "removing the publisher stops trusting it");
check(problems.length === 0, "no console errors: " + problems.join("; "));
await browser.close();
server.kill();
process.exit(process.exitCode ?? 0);
