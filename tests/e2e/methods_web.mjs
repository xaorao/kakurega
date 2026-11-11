// Methods as a person uses them, with a real local model: write one in Instructions, see it wait for approval, read it,
// approve it, ask in Chat (the answer says it was used), edit it (approval is gone), and refuse a method with a power field.
// Needs Ollama with the model given (default qwen2.5:3b) and `cargo build -p kk-cli`.
//
//   node tests/e2e/methods_web.mjs [MODEL]     screenshots in target/methods-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/methods-web");
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
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1000 } })).newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error" && !/status of 422/.test(m.text())) problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => { if (!/ERR_ABORTED/.test(r.failure()?.errorText ?? "")) problems.push("requestfailed: " + r.url()); });
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { if (!cond) { console.log("FAIL", what); process.exitCode = 1; } else console.log("PASS", what); };
const origin = new URL(link).origin;

await page.goto(link);
await page.waitForSelector("nav");
await page.goto(origin + "/#/skills");
await page.getByRole("heading", { name: "Methods" }).waitFor();
check(await page.getByText("No methods yet.").isVisible(), "a new install has no methods and says so");
await page.locator("#met-new-name").fill("fruit-first");
await page.locator("#met-new-desc").fill("Start answers with a fruit");
await page.locator("#met-new-trig").fill("pineapple, orchard");
await page.locator("#met-new-body").fill("Begin your answer with the single word PINEAPPLE, then answer normally.");
await page.getByRole("button", { name: /^Save \(it will wait/ }).click();
await page.getByText("Saved. Read it, then approve it to use it.").waitFor();
await shot("1-saved-waiting");
check(await page.getByText("Waiting for your approval: not in use").isVisible(), "a saved method waits for approval");

const ask = async (text) => {
  await page.goto(origin + "/#/chat");
  await page.getByLabel(/Message|Ask/i).first().fill(text);
  await page.getByRole("button", { name: "Send" }).click();
  await page.waitForFunction(() => /answered on this computer|tokens of context used|No model was asked/.test(document.querySelector("main")?.innerText ?? ""), null, { timeout: 240000 });
  const all = await page.locator("main").innerText();
  return all.split(text).pop();
};
let body = await ask("Tell me about a pineapple orchard in one sentence.");
await shot("2-chat-before-approval");
check(!/Methods used/.test(body), "an unapproved method is not used and the answer does not say it was");

await page.goto(origin + "/#/skills");
await page.getByRole("heading", { name: "Methods" }).waitFor();
await page.getByRole("button", { name: "Show text" }).click();
check(await page.getByText("Begin your answer with the single word PINEAPPLE").isVisible(), "the owner can read the exact text before approving");
await page.getByRole("button", { name: "Approve this text" }).click();
await page.getByText("Approved: in use").waitFor();
await shot("3-approved");

body = await ask("Tell me about a pineapple orchard in one sentence.");
await shot("4-chat-after-approval");
check(/Methods used: fruit-first/.test(body), "an approved, matching method is used and the answer says so");
console.log("OBSERVED reply starts with PINEAPPLE:", /PINEAPPLE/i.test(body), "(a small model may ignore guidance; not asserted)");
body = await ask("What is two plus two?");
check(!/Methods used/.test(body), "a question with none of its words does not use it");

await page.goto(origin + "/#/skills");
await page.getByRole("heading", { name: "Methods" }).waitFor();
await page.getByRole("button", { name: "Edit" }).click();
await page.getByLabel("The text").first().fill("Begin your answer with the single word MANGO, then answer normally.");
await page.getByRole("button", { name: /^Save \(it will wait/ }).first().click();
await page.getByText("Waiting for your approval: not in use").waitFor();
check(true, "editing the text takes the approval away");
await shot("5-edited-waiting");
body = await ask("Tell me about a pineapple orchard in one sentence.");
check(!/Methods used/.test(body), "the edited method is not used until approved again");

const refused = await page.evaluate(async () => {
  const csrf = (await (await fetch("/api/session", { credentials: "same-origin" })).json()).csrf;
  const r = await fetch("/api/methods", { method: "POST", headers: { "Content-Type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify({ name: "x", description: "d", triggers: ["alpha"], body: "t", tools: ["shell"] }) });
  return r.status;
});
check(refused === 422, `a method with a tools field is refused with 422 (status ${refused})`);

check(problems.length === 0, "no console errors: " + problems.join("; "));
await browser.close();
server.kill();
