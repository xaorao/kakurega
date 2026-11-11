// Names hidden from a model that is not on this computer, as a person does it: list a name in Settings, ask in Chat, read the
// answer with the real values, open "What was sent to the model" and see placeholders there.
// This needs a model elsewhere, so it runs only when ANTHROPIC_API_KEY is set (the outside test route); otherwise it says so.
//   ANTHROPIC_API_KEY=... node tests/e2e/hide_web.mjs      screenshots in target/hide-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

if (!process.env.ANTHROPIC_API_KEY) {
  console.log("SKIP hide_web: no ANTHROPIC_API_KEY, so there is no model elsewhere to hide names from");
  process.exit(0);
}
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/hide-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const model = process.env.KK_OUTSIDE_MODEL || "anthropic:claude-sonnet-5-5";
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--route", "outside", "--outside-model", model, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1100 } })).newPage();
const errors = [];
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
page.on("pageerror", (e) => errors.push(String(e)));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();

await page.goto(new URL(link).origin + "/#/settings");
await page.waitForSelector("#hide-name");
check((await page.locator("#hide-applies").innerText()).includes("not on this computer"), "Settings says the list is in use because the model is elsewhere");
for (const name of ["Asha Rao", "Meridian Capital"]) {
  await page.fill("#hide-name", name);
  await page.getByRole("button", { name: "Hide it" }).click();
  await page.getByText("Added. It is replaced by a placeholder").waitFor();
}
check((await page.getByRole("list", { name: "Names that are hidden" }).innerText()).includes("Meridian Capital"), "both names are listed");
await page.fill("#hide-name", "x[1]");
await page.getByRole("button", { name: "Hide it" }).click();
await page.getByRole("alert").filter({ hasText: "square brackets" }).waitFor();
check(true, "a name with brackets is refused in words");
await page.screenshot({ path: join(work, "1-settings.png"), fullPage: true });

await page.goto(new URL(link).origin + "/#/chat");
await page.waitForSelector("#chat-input");
const question = "Write one short sentence that thanks Asha Rao of Meridian Capital for the call and says I will email asha.rao@meridian.example tomorrow. Use the name and the address exactly.";
await page.fill("#chat-input", question);
await page.keyboard.press("Enter");
const foot = page.getByText(/personal values? (were|was) sent as/);
await foot.waitFor({ timeout: 120000 });
const body = await page.locator("main").innerText();
check(/3 personal values were sent as placeholders/.test(body), "the reply says three values went as placeholders");
check(body.includes("Asha Rao") && body.includes("asha.rao@meridian.example") && body.split("Meridian Capital").length > 2, "the answer the owner reads has the real name, company and address");
check(!/\[\[(NAME|EMAIL)_\d+\]\]/.test(body.replace(question, "")), "no placeholder is left in the answer");
await page.getByText("What was sent to the model").last().click();
const sent = page.locator("details").filter({ hasText: "What was sent to the model" }).last();
await sent.getByText("[[NAME_1]]").first().waitFor();
const sentText = await sent.innerText();
check(sentText.includes("[[NAME_1]]") && sentText.includes("[[NAME_2]]") && sentText.includes("[[EMAIL_1]]"), "what was sent shows the placeholders");
check(!/Asha|Rao|Meridian|meridian\.example/.test(sentText), "what was sent holds none of the real values");
check(sentText.includes("not word for word"), "the view says the sent words differ from what was typed");
await page.screenshot({ path: join(work, "2-chat.png"), fullPage: true });
// the refused name is a 422 the browser reports by itself; anything else is a defect
const other = errors.filter((e) => !/status of 422/.test(e));
check(errors.length - other.length === 1 && other.length === 0, "no console errors beyond the one refusal" + (other.length ? ": " + other.join(" | ") : ""));
await browser.close();
server.kill();
