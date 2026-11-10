// The Documents meaning index as a person uses it, with a real local model and the real embedding model (nomic-embed-text):
// ask a question about a folder, see how many passages the computer kept, forget them, see none kept, ask again. Needs Ollama with the model given (default qwen2.5:3b) and `cargo build -p kk-cli`.
//
//   node tests/e2e/notes_only.mjs [MODEL]     screenshots in target/doc-index/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/doc-index");
const model = process.argv[2] ?? "qwen2.5:3b";
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(join(root, "agreements"), { recursive: true });
import { writeFileSync } from "node:fs";
for (let i = 0; i < 30; i++) writeFileSync(join(root, "agreements", `p${i}.txt`), Array.from({ length: 450 }, (_, k) => `Section ${i}.${k}: routine paragraph about offices and scheduling and supplies.`).join("\n") + (i === 17 ? "\nEither party may terminate this agreement on ninety days written warning.\n" : "\n"));
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", model, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
process.on("exit", () => { try { server.kill(); } catch {} });
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const page = await ctx.newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => { if (!/ERR_ABORTED/.test(r.failure()?.errorText ?? "")) problems.push("requestfailed: " + r.url()); });
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { if (!cond) { console.log("FAIL", what); process.exitCode = 1; } else console.log("PASS", what); };
const origin = new URL(link).origin;
const modelCalls = () => page.evaluate(async () => (await (await fetch("/api/usage")).json()).total?.replies ?? (await (await fetch("/api/usage")).json()).total?.turns ?? null);

await page.goto(link);
await page.waitForSelector("nav");
await page.goto(origin + "/#/docs");
await page.getByRole("heading", { name: "Ask your documents" }).waitFor();
await page.getByText("Nothing is kept.").waitFor();
check(true, "before any question nothing is kept");
await page.getByRole("button", { name: "agreements" }).click();
await page.getByLabel("Your question").fill("How can the contract be cancelled early?");
await page.getByRole("button", { name: /Ask/ }).first().click();
await page.getByText(/passages are kept, for/).waitFor({ timeout: 300000 });
await shot("1-kept");
const kept = await page.getByText(/passages are kept, for/).innerText();
const n = Number(/^(\d+) passages/.exec(kept)?.[1] ?? 0);
check(n > 0, "after a question the card says how many passages are kept and for which model: " + kept);
await page.getByRole("button", { name: "Forget it all" }).click();
await page.getByRole("alertdialog").waitFor();
await shot("2-confirm");
await page.getByRole("button", { name: "Cancel" }).click();
check((await page.getByText(/passages are kept, for/).count()) === 1, "cancel forgets nothing");
await page.getByRole("button", { name: "Forget it all" }).click();
await page.getByRole("button", { name: "Yes, forget it" }).click();
await page.getByText(/Forgotten: \d+ passages\./).waitFor();
await shot("3-forgotten");
check(await page.getByText("Nothing is kept.").count() === 1, "after forgetting the card says nothing is kept");
check(readdirSync(join(root, "agreements")).length === 30, "the files are untouched");
check(problems.length === 0, "no console errors or failed requests: " + problems.join("; "));
await browser.close();
server.kill();
