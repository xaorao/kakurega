// The project "answer only from this project's notes and files" switch as a person uses it, with a real local model:
// turn it on in Instructions, ask in Chat with nothing saved (no model is asked), save a note, ask again (the model answers
// from the note), turn it off. Needs Ollama with the model given (default qwen2.5:3b) and `cargo build -p kk-cli`.
//
//   node tests/e2e/notes_only.mjs [MODEL]     screenshots in target/notes-only/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/notes-only");
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
await page.goto(origin + "/#/instructions");
await page.getByRole("heading", { name: "Instructions" }).waitFor();
await page.getByLabel(/Add instructions for another project/).fill("kestrel");
const box = page.getByLabel("Answer only from this project's notes and attached files");
await box.check();
await page.getByText("On for kestrel").waitFor();
await shot("1-switch-on");
check(await box.isChecked(), "the switch is on and says so");
await page.reload();
await page.goto(origin + "/#/instructions");
await page.getByRole("heading", { name: "Instructions" }).waitFor();
await page.getByLabel("Answer only from this project's notes and attached files").first().waitFor();
check(await page.getByLabel("Answer only from this project's notes and attached files").first().isChecked(), "the choice is still on after a reload");

const ask = async (text) => {
  await page.goto(origin + "/#/chat");
  await page.locator("#chat-project").fill("kestrel");
  await page.getByLabel(/Message|Ask/i).first().fill(text);
  await page.getByRole("button", { name: "Send" }).click();
};
await ask("What is the rounding rule?");
await page.getByText("Nothing in this project").waitFor({ timeout: 20000 });
await shot("2-nothing-here");
const body1 = await page.locator("main").innerText();
check(/Nothing in this project's notes or attached files answers that/.test(body1), "with nothing saved the reply says so plainly");
check(/No model was asked/.test(body1) && !/answered on this computer|tokens of context used/.test(body1.split("Nothing in this project")[1] ?? ""), "the footer says no model was asked and shows no model figures");

await page.goto(origin + "/#/memory");
await page.getByRole("heading", { name: /Memory|Notes/ }).first().waitFor();
await page.getByLabel(/^(Note|What should)/i).first().fill("The rounding rule is banker's rounding to two decimals.");
const projectField = page.getByLabel(/Project/i).first();
if (await projectField.count()) await projectField.fill("kestrel");
await page.getByRole("button", { name: /^(Save|Remember|Add)/ }).first().click();
await page.waitForTimeout(500);
await ask("What is the rounding rule?");
await page.waitForFunction(() => !document.querySelector('[aria-busy="true"]') && !/Working|Thinking|Stop/.test(document.querySelector("main")?.innerText ?? ""), null, { timeout: 180000 }).catch(() => {});
await shot("3-answered-from-the-note");
const answer = await page.locator("main").innerText();
console.log("--- screen text after the second question ---\n" + answer.slice(-900));
check(/banker|two decimals/i.test(answer.replace(/Notes used[\s\S]*$/i, "")), "with a note saved the model answers from it");

await page.goto(origin + "/#/instructions");
await page.getByLabel("Answer only from this project's notes and attached files").first().uncheck();
await page.getByText("Off for kestrel").waitFor();
check(true, "the switch turns off");
check(problems.length === 0, "no console errors or failed requests: " + problems.join("; "));
await browser.close();
server.kill();
