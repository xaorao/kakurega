// Saved prompts as a person uses them in Chat: write, name, save, reload, use with one click, remove; a prompt that holds
// a key is refused.
//   node tests/e2e/prompts_web.mjs      screenshots in target/prompts-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/prompts-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 24000 + (process.pid % 1000);
const procs = [];
process.on("exit", () => { for (const p of procs) { try { p.kill(); } catch {} } });
procs.push(spawn("python3", [join(repo, "tests/e2e/team_script.py"), String(port)], { stdio: "ignore" }));
await new Promise((r) => setTimeout(r, 600));
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", join(work, "root"), "--model", "team-script", "--ollama", `http://127.0.0.1:${port}`, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
procs.push(server);
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1000 } })).newPage();
const errors = [];
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
page.on("pageerror", (e) => errors.push(String(e)));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.getByRole("link", { name: "Chat", exact: true }).click();
await page.waitForSelector("#chat-input");
await page.locator("#saved-prompts summary").click();
check((await page.locator("#saved-prompts").innerText()).includes("No saved prompts yet"), "it starts empty and says how to save one");
check(await page.getByRole("button", { name: "Save as a prompt" }).isDisabled(), "nothing can be saved from an empty box");
await page.fill("#chat-input", "Summarise this week in five plain bullets.");
await page.fill("#prompt-name", "Weekly summary");
await page.getByRole("button", { name: "Save as a prompt" }).click();
await page.locator("#saved-prompts").getByText("Saved.").waitFor();
check((await page.locator("#saved-prompts summary").innerText()).includes("(1)"), "the prompt is saved and counted");

const key = ["sk-ant", "api03-abcdefghijklmnopqrstuvwxyz0123456789"].join("-");
await page.fill("#chat-input", `Use the key ${key} for the export.`);
await page.fill("#prompt-name", "With a key");
await page.getByRole("button", { name: "Save as a prompt" }).click();
await page.locator("#saved-prompts").getByRole("alert").waitFor();
check((await page.locator("#saved-prompts").getByRole("alert").innerText()).includes("looks like a secret"), "a prompt that holds a key is refused in words");

await page.reload();
await page.waitForSelector("#chat-input");
await page.fill("#chat-input", "");
await page.locator("#saved-prompts summary").click();
await page.getByRole("button", { name: "Weekly summary", exact: true }).click();
check((await page.inputValue("#chat-input")) === "Summarise this week in five plain bullets.", "after a reload, one click puts the saved prompt into the box");
check(await page.evaluate(() => document.activeElement?.id === "chat-input"), "and the box has the keyboard");
// found by using it: a name that differs only in capitals replaces the old prompt, and the page says so
await page.fill("#chat-input", "Summarise this week in three bullets.");
await page.fill("#prompt-name", "weekly SUMMARY");
await page.getByRole("button", { name: "Save as a prompt" }).click();
await page.locator("#saved-prompts").getByText(/It replaced the prompt that was already called/).waitFor({ timeout: 10000 });
check(true, "saving under a name that differs only in capitals says it replaced the earlier prompt");
await page.fill("#chat-input", "");
await page.screenshot({ path: join(work, "1-prompts.png"), fullPage: true });
// a prompt with fields: each is asked for, the box is filled only when all are, and the date fills itself
await page.fill("#chat-input", "Write to {{client}} in a {{tone:formal|casual}} tone about the {{client}} renewal. Date it {{current_date}}.");
await page.fill("#prompt-name", "Client letter");
await page.getByRole("button", { name: "Save as a prompt" }).click();
await page.locator("#saved-prompts").getByText("Saved.").waitFor();
await page.fill("#chat-input", "");
await page.getByRole("button", { name: "Client letter", exact: true }).click();
check(await page.getByLabel("client", { exact: true }).isVisible() && await page.getByLabel("tone", { exact: true }).isVisible() && (await page.locator(".prompt-vars input").count()) === 2, "each field is asked for once, and the date is not asked for");
check((await page.inputValue("#chat-input")) === "", "nothing goes into the box before the fields are filled");
await page.getByLabel("client", { exact: true }).fill("Acme");
await page.getByRole("button", { name: "Put it in the message box" }).click();
check((await page.locator("#saved-prompts").getByRole("alert").innerText()).includes("Fill in tone first."), "an empty field is named, and nothing is put in the box");
await page.getByLabel("tone", { exact: true }).fill("formal");
await page.screenshot({ path: join(work, "2-fields.png"), fullPage: true });
await page.getByRole("button", { name: "Put it in the message box" }).click();
const today = new Date();
const two = (n) => String(n).padStart(2, "0");
const date = `${today.getFullYear()}-${two(today.getMonth() + 1)}-${two(today.getDate())}`;
check((await page.inputValue("#chat-input")) === `Write to Acme in a formal tone about the Acme renewal. Date it ${date}.`, "the box holds the prompt with every field filled", await page.inputValue("#chat-input"));
await page.locator("#saved-prompts").getByRole("button", { name: /Remove.*Client letter/ }).click();
await page.locator("#saved-prompts").getByText("Removed.").waitFor();
await page.locator("#saved-prompts").getByRole("button", { name: /Remove/ }).click();
await page.locator("#saved-prompts").getByText("Removed.").waitFor();
check((await page.locator("#saved-prompts summary").innerText()).includes("(0)"), "removing it empties the list");
const other = errors.filter((e) => !/status of 422/.test(e));
check(other.length === 0, "no console errors beyond the one refusal" + (other.length ? ": " + other.join(" | ") : ""));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
