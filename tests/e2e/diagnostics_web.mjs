// Settings > Report a problem as a person uses it, against the real service:
// show the report, read it, copy it, save it as a file; it names nothing private. Needs Ollama with the model given (default qwen2.5:3b) and `cargo build -p kk-cli`.
//
//   node tests/e2e/notes_only.mjs [MODEL]     screenshots in target/diag-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/diag-web");
const model = process.argv[2] ?? "qwen2.5:3b";
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
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
const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, permissions: ["clipboard-read", "clipboard-write"], acceptDownloads: true });
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
await page.goto(origin + "/#/settings");
await page.getByRole("heading", { name: "Report a problem" }).waitFor();
check((await page.locator("pre.passage").count()) === 0, "nothing is shown until asked");
await page.getByRole("button", { name: "Show the report" }).click();
const pre = page.locator("pre.passage");
await pre.waitFor();
const text = await pre.innerText();
await shot("1-report");
check(text.startsWith("Kakurega diagnostics") && /Checks/.test(text) && /Counts/.test(text) && /Crashes recorded/.test(text), "the report reads in plain words");
check(text.includes("This report holds no file names"), "it says what it leaves out");
check(!text.includes(homedir()) && !text.includes(work) && !/\/home\/|\/Users\//.test(text), "it names no home folder, user name or data folder");
await page.getByRole("button", { name: "Copy the report" }).click();
await page.getByText("Copied.").waitFor();
check((await page.evaluate(() => navigator.clipboard.readText())) === text.replace(/\n$/, "") || (await page.evaluate(() => navigator.clipboard.readText())).startsWith("Kakurega diagnostics"), "copy puts the same text on the clipboard");
const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("button", { name: "Save as a file" }).click()]);
check(download.suggestedFilename() === "kakurega-diagnostics.txt", "save offers a text file");
await page.getByRole("button", { name: "Hide the report" }).click();
check((await page.locator("pre.passage").count()) === 0, "hiding removes it");
check(problems.length === 0, "no console errors or failed requests: " + problems.join("; "));
await browser.close();
server.kill();
