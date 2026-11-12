// A question about a project's code, as a person asks it: from the project's card, with the scripted test model and the
// real sandbox. The answer's lines are checked against the file (one is found, one the model made up is not), the script
// the agent wrote runs in the sandbox without the withheld file, and the question is kept and can be forgotten.
//   node tests/e2e/explore_web.mjs      screenshots in target/explore-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/explore-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const shop = join(root, "shop");
mkdirSync(join(shop, "app"), { recursive: true });
writeFileSync(join(shop, "app/session.py"), "SESSIONS = {}  # \u200b\u202e} = SNOISSES\n\n\ndef logout(token):\n    # the token is forgotten here\n    return True\n");
writeFileSync(join(shop, "app/__init__.py"), "");
// a project file that would run before any script and fake its result, and one that would stand in for a standard module
writeFileSync(join(shop, "sitecustomize.py"), "print('FAKED: all checks passed')\nimport sys\nsys.exit(0)\n");
writeFileSync(join(shop, "json.py"), "raise SystemExit('FAKED json')\n");
const key = ["sk_li", "ve_abcdefghijklmnop0123"].join("");
writeFileSync(join(shop, ".env"), `STRIPE_KEY=${key}\n`);
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const shots = join(work, "shots");
mkdirSync(shots, { recursive: true });
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--scripted", join(repo, "tests/e2e/script.json"), "--ui-dir", join(repo, "apps/web/dist")]);
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1300 } });
const page = await ctx.newPage();
const errors = [];
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
page.on("pageerror", (e) => errors.push(String(e)));
const check = (cond, what, detail = "") => { console.log(cond ? "PASS" : "FAIL", what, cond ? "" : detail); if (!cond) process.exitCode = 1; };

try {
  await page.goto(link);
  await page.waitForSelector("nav");
  const skip = page.getByRole("button", { name: "Skip the rest" });
  if (await skip.count()) await skip.click();

  // from the project's card
  await page.getByRole("link", { name: "Projects" }).click();
  await page.locator("#proj-name").fill("shop");
  await page.locator("#proj-folder").fill(shop);
  await page.getByRole("button", { name: "Save the project" }).click();
  await page.getByRole("button", { name: "Ask about its code" }).click();
  await page.getByRole("heading", { name: "Ask a project", exact: true }).waitFor();
  await page.waitForFunction((p) => document.querySelector(".path")?.textContent?.endsWith(p), "/shop");
  check((await page.locator(".path").innerText()).endsWith("/shop"), "the project's folder is already chosen");
  const ask = page.getByRole("button", { name: "Ask", exact: true });
  check(await ask.isDisabled(), "nothing can be asked with an empty box");

  // a question that holds a password is refused before anything runs
  await page.getByLabel("Your question").fill("the password for the staging router is Blue42Harbor; where is it used?");
  await ask.click();
  await page.getByRole("alert").filter({ hasText: "looks like a secret" }).waitFor();
  check(true, "a question holding a password is refused, in words");
  // that refusal is a 400 the browser logs as a failed request; it is the expected answer, and nothing else may fail
  check(errors.length === 1 && /status of 400/.test(errors[0]), "the refusal is the only failed request so far", errors.join(" | "));
  errors.length = 0;

  await page.getByLabel("Your question").fill("Does logout revoke the session token?");
  await page.getByRole("radio", { name: /Thorough/ }).check();
  await ask.click();
  await page.getByText(/of the 2 lines the answer rests on were found/).waitFor({ timeout: 120000 });
  await page.screenshot({ path: join(shots, "answered.png"), fullPage: true });
  const card = await page.locator("section.card", { has: page.getByRole("heading", { name: "Does logout revoke the session token?" }) }).innerText();
  check(card.includes("1 of the 2 lines the answer rests on were found in the files."), "one line is found and one is not, and the count says so", card.slice(0, 400));
  check(/found at that line/.test(card) && /not in the file/.test(card), "each line says what was found");
  check(card.includes("SESSIONS = {}"), "a found line is shown as the file has it");
  check(card.includes("this line was invented by the scripted test model") && card.includes("The agent wrote:"), "a line not in the file is shown as the agent's words, not as code");
  check(card.includes("Some lines were not found at first; the agent was told and answered again."), "the correction round is said");
  check(card.includes("Scripted test answer"), "the answer is the agent's words, labelled");
  check(!card.includes(key), "the key in the withheld file appears nowhere");
  check(/1 file was withheld from the agent/.test(card), "the withheld file is counted");
  await page.getByText(/Check 1 \(Python\)/).click();
  const after = await page.locator("section.card", { has: page.getByRole("heading", { name: "Does logout revoke the session token?" }) }).innerText();
  check(/Check 1 \(Python\) · ended without an error/.test(after), "the script the agent wrote ran in the sandbox", after.slice(-600));
  check(/no \.env file is visible/.test(after), "the withheld file was not given to the script", after.slice(-600));
  check(!/FAKED/.test(after), "a project file cannot run before the script or replace a standard module", after.slice(-600));
  const passages = await page.locator("pre.passage").allTextContents();
  check(passages.some((t) => t.includes("\ufffd")) && !passages.some((t) => /[\u200b\u202e]/.test(t)), "hidden and direction-changing characters in a cited line are shown as marks", JSON.stringify(passages).slice(0, 300));
  check(/1 file read/.test(after), "the files the agent read are listed");
  await page.screenshot({ path: join(shots, "check-open.png"), fullPage: true });

  // kept, listed as earlier, forgotten
  await page.reload();
  await page.waitForSelector("nav");
  await page.getByRole("link", { name: "Ask a project" }).click();
  await page.getByRole("heading", { name: "Earlier questions" }).waitFor();
  const earlier = page.getByRole("button", { name: "Does logout revoke the session token?" });
  check(await earlier.count() === 1 && /1 of 2 lines found/.test(await page.locator("#h-ex-earlier").locator("xpath=..").innerText()), "the question is kept with its count");
  await earlier.click();
  await page.getByRole("button", { name: "Forget this question" }).click();
  await page.waitForFunction(() => !document.querySelector("#h-ex-earlier"));
  check(true, "a forgotten question is gone");
  check(errors.length === 0, "no console or page errors", errors.join(" | "));
} catch (e) {
  check(false, "the walk ran to its end", String(e).slice(0, 400));
  await page.screenshot({ path: join(shots, "failure.png"), fullPage: true }).catch(() => {});
} finally {
  await browser.close();
  server.kill();
}
