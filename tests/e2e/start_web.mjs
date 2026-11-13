// The start screen as a person uses it: a greeting, one box to ask in (the question is sent in Chat), and four ways to begin.
//   node tests/e2e/start_web.mjs      screenshots in target/start-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { cpSync, mkdirSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/start-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), join(work, "root/payments-service"), { recursive: true });
renameSync(join(work, "root/payments-service/_env"), join(work, "root/payments-service/.env"));
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", join(work, "root"), "--scripted", join(repo, "tests/e2e/script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const origin = new URL(link).origin;
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const page = await ctx.newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.getByRole("button", { name: "Skip the rest" }).click({ timeout: 6000 }).catch(() => {});
await page.goto(origin + "/#/");
const hello = await page.locator("h1.start-hello").innerText();
check(/^(Good morning|Good afternoon|Good evening|Working late)\.$/.test(hello), "the start greets by the time of day: " + hello);
const box = page.getByLabel("Ask something");
const askButton = page.getByRole("button", { name: "Ask", exact: true });
check(await askButton.isDisabled(), "with nothing typed there is nothing to ask");
check((await page.locator(".start-ask-row .muted").innerText()).length > 10, "under the box it says where the answer will come from");
const main = (await page.locator("#side-nav > a.navlink").allInnerTexts()).map((s) => s.trim());
check(main.length === 6 && main.join("|") === "Waiting for you|Chat|Ask your documents|Projects|Team|Memory", "the sidebar shows six places and folds the rest: " + main.join("|"));
check(!(await page.getByRole("link", { name: "Activity" }).isVisible()), "the rest is folded under More");
await page.locator("#nav-more > summary").click();
check(await page.getByRole("link", { name: "Activity" }).isVisible(), "and one click shows it");
await page.locator("#nav-more > summary").click();
await page.screenshot({ path: join(work, "1-start.png") });

// a question typed here is asked in Chat, once
await box.fill("Where are login sessions kept?");
await box.press("Enter");
await page.waitForURL(/#\/chat/);
await page.locator(".msg.assistant").first().waitFor({ timeout: 30000 });
const said = await page.locator(".msg.user").allInnerTexts();
check(said.length === 1 && /Where are login sessions kept\?/.test(said[0]), "the question is asked in Chat, once: " + said.length);
check((await page.getByLabel("Your message").inputValue()) === "", "and the message box is empty again");
// the box you type in stays in reach while the conversation is read
const stuck = await page.evaluate(() => { const f = document.querySelector("form.composer"); const r = f.getBoundingClientRect(); return { bottom: Math.round(innerHeight - r.bottom), height: Math.round(r.height), position: getComputedStyle(f).position }; });
check(stuck.position === "sticky" && stuck.bottom >= 0 && stuck.height <= 280, "the message box is in the window, kept at its bottom while reading, and small: " + JSON.stringify(stuck));
await page.screenshot({ path: join(work, "2-chat.png") });
await page.goBack();
await page.waitForURL((u) => !/#\/chat/.test(u.href));
await page.goto(origin + "/#/chat");
await page.waitForTimeout(600);
check((await page.locator(".msg.user").count()) <= 1, "coming back to Chat does not ask it again");

// the ways to begin
await page.goto(origin + "/#/");
await page.getByRole("button", { name: /^Review a change/ }).click();
check(await page.getByLabel("Only a change (a branch against its base, as for a pull request)").isChecked(), "Review a change opens the review form with a change chosen");
await page.getByRole("button", { name: /^Review a folder/ }).click();
check(await page.getByLabel("The whole folder").isChecked(), "Review a folder chooses the whole folder");
await page.getByRole("link", { name: /^Ask your documents Answers/ }).click();
await page.waitForURL(/#\/docs/);
check(true, "Ask your documents opens that screen");
await page.goto(origin + "/#/");
await page.getByRole("link", { name: /^Meeting notes Decisions/ }).click();
await page.waitForURL(/#\/meeting/);
check(true, "Meeting notes opens that screen");

// the owner's saved prompts: none on a new install, so no row; once saved, a chip fills the box and sends nothing
await page.goto(origin + "/#/");
await page.getByRole("button", { name: /^Review a folder/ }).waitFor();
check((await page.getByRole("group", { name: "Your saved prompts" }).count()) === 0, "with no saved prompts the start screen shows no row for them");
const saved = await page.evaluate(async () => {
  const csrf = (await (await fetch("/api/session", { credentials: "same-origin" })).json()).csrf;
  const r = await fetch("/api/prompts", { method: "POST", headers: { "Content-Type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify({ name: "Weekly risk note <b>", text: "List this week's three biggest risks in the payments service." }) });
  return r.status;
});
check(saved === 200, "a prompt was saved through the service: " + saved);
await page.reload();
const chip = page.getByRole("group", { name: "Your saved prompts" }).getByRole("button", { name: "Weekly risk note <b>" });
await chip.waitFor({ timeout: 10000 });
check((await page.locator(".start-mine b").count()) === 0, "a prompt's name is shown as text, never as markup");
const before = await page.locator(".msg.user").count();
await chip.click();
check((await page.locator("#start-ask").inputValue()) === "List this week's three biggest risks in the payments service.", "choosing a saved prompt fills the box with its text");
check(/#\/$|#$|\/$/.test(new URL(page.url()).hash || "#/") && (await page.locator(".msg.user").count()) === before, "and nothing is sent until the owner asks");

// a phone
const phone = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "dark" });
const pp = await phone.newPage();
const code = /#pair=([\w-]+)/.exec(link)[1];
await pp.goto(origin + "/#/");
await pp.screenshot({ path: join(work, "3-phone.png") });
await phone.close();
check(problems.length === 0, "no console errors: " + problems.join(" | "));
await browser.close();
server.kill();
