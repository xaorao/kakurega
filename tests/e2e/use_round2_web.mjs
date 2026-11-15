// Fixes from the second hunt, driven in a real browser against the real service (scripted model): leaving Chat while an
// answer is being written says so and puts the question back (T4); Esc that closes the command palette does not stop the
// answer (T5); saving a prompt under a name that differs only in case says it replaced the old one (T9); a patch whose
// tests fail is never offered as ready (T3); an open look-alike panel follows notes added on the screen (T7); a project
// question still being answered is picked up again (T6). A slow model is stood in for by holding the answer's request in
// the browser, and the code task and question answers (which need a real model) are stood in for in the browser; the
// checks are on what the page shows and offers, waiting on the page, never on the clock.
//
//   node tests/e2e/use_round2_web.mjs      screenshots in target/use-round2-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/use-round2-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
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
page.on("requestfailed", (r) => { if (!/ERR_ABORTED/.test(r.failure()?.errorText ?? "")) problems.push("requestfailed: " + r.url()); });
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

// the answer's request is held until `release()` is called: a model that is still writing
let release = () => {};
const hold = () => page.route("**/api/chat/stream", async (route) => {
  await new Promise((r) => { release = r; });
  await route.continue().catch(() => undefined);
});

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.goto(origin + "/#/chat");
const box = page.getByLabel("Your message");
await box.waitFor();

// ---- T4: leaving while an answer is written ----
await hold();
await box.fill("What changed in the payments service this week?");
await page.getByRole("button", { name: "Send", exact: true }).click();
await page.getByRole("button", { name: "Stop", exact: true }).waitFor();
await page.goto(origin + "/#/memory");
await page.getByRole("heading", { level: 1 }).first().waitFor();
release();
await page.unroute("**/api/chat/stream");
await page.goto(origin + "/#/chat");
await page.getByText("You left while an answer was being written, so it was stopped.").waitFor({ timeout: 10000 }).catch(() => undefined);
check(await page.getByText("You left while an answer was being written, so it was stopped.").isVisible(), "coming back to Chat after leaving mid-answer says the answer was stopped");
check((await page.getByLabel("Your message").inputValue()) === "What changed in the payments service this week?", "and the question is back in the box, ready to send again");
await shot("1-left-mid-answer");
await page.getByRole("button", { name: "Send", exact: true }).click();
await page.getByText("This is a scripted reply, not a real model.").last().waitFor({ timeout: 15000 });
check(!(await page.getByText("You left while an answer was being written").isVisible()), "sending again clears the note and the answer arrives");

// ---- T5: Esc on the palette does not stop an answer ----
await hold();
await box.fill("Second question, answered while the palette opens and closes.");
await page.getByRole("button", { name: "Send", exact: true }).click();
await page.getByRole("button", { name: "Stop", exact: true }).waitFor();
await page.keyboard.press(process.platform === "darwin" ? "Meta+k" : "Control+k");
const palette = page.getByRole("dialog");
await palette.waitFor();
await page.keyboard.press("Escape");
await palette.waitFor({ state: "detached", timeout: 5000 }).catch(() => palette.waitFor({ state: "hidden", timeout: 5000 }));
check(await page.getByRole("button", { name: "Stop", exact: true }).isVisible(), "Esc closes the palette and the answer keeps being written");
release();
await page.unroute("**/api/chat/stream");
await page.getByText("This is a scripted reply, not a real model.").nth(1).waitFor({ timeout: 15000 });
check((await page.getByText("This is a scripted reply, not a real model.").count()) >= 2, "and the answer arrives whole");
await shot("2-esc-palette");

// ---- T9: a saved prompt name that differs only in case replaces, and says so ----
await box.fill("Summarise the open review findings for the weekly report.");
const promptsPanel = page.getByText(/^Saved prompts \(\d+\)$/).first();
if (await promptsPanel.count()) await promptsPanel.click();
await page.getByLabel("Name for what is in the box").fill("Weekly report");
await page.getByRole("button", { name: "Save as a prompt" }).click();
await page.getByText("Saved.", { exact: true }).waitFor({ timeout: 5000 });
await box.fill("Summarise the delivered releases for the weekly report.");
await page.getByLabel("Name for what is in the box").fill("weekly REPORT");
await page.getByRole("button", { name: "Save as a prompt" }).click();
await page.getByText(/It replaced the prompt that was already called/).waitFor({ timeout: 5000 }).catch(() => undefined);
check(await page.getByText(/It replaced the prompt that was already called/).isVisible(), "saving under a name that differs only in case says it replaced the old prompt");
const saved = await page.evaluate(async () => (await (await fetch("/api/prompts", { credentials: "same-origin" })).json()).prompts);
check(saved.length === 1 && /delivered releases/.test(saved[0].text), "and only the new text is kept: " + JSON.stringify(saved.map((p) => p.name)));
await shot("3-prompt-case");

// ---- T3: a patch whose tests fail is not offered as ready ----
// the coding agent needs a real model; its finished task is stood in for, so the check is on what the page offers
const failing = { id: "code-1", task: "Fix pricing.py", folder: join(work, "root"), state: "ready", error: null, attention: [], progress: ["read 3 files", "changed pricing.py", "ran the tests"], ended: null, summary: "Changed the rounding.", tests: { ran: true, passed: false, output: "FAILED test_pricing.py::test_discount - assert 9 == 10" }, test_command: "python3 -m pytest -q", withheld_files: 0, steps: [], changes: [{ path: "pricing.py", kind: "modified", diff: "--- a/pricing.py\n+++ b/pricing.py\n@@ -1 +1 @@\n-return round(x)\n+return int(x)\n", added: 1, removed: 1 }], digest: "ab".repeat(32), concerns: [], can_apply: true };
await page.route("**/api/code", (route) => route.request().method() === "GET" ? route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ tasks: [{ id: "code-1", task: "Fix pricing.py", state: "ready", created: 1, tests_fail: true }] }) }) : route.continue());
await page.route("**/api/code/code-1", (route) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(failing) }));
await page.goto(origin + "/#/code");
await page.getByRole("heading", { name: "Earlier tasks" }).waitFor();
check(await page.getByText("tests fail", { exact: true }).isVisible(), "an earlier task whose tests fail is listed as \"tests fail\", not as ready");
await page.getByRole("button", { name: "Open", exact: true }).click();
await page.getByText("The tests fail with this change, so it is probably wrong.").waitFor({ timeout: 10000 }).catch(() => undefined);
check(await page.getByText("The tests fail with this change, so it is probably wrong.").isVisible(), "the open task says its tests fail and it is probably wrong");
check((await page.getByRole("button", { name: "Apply anyway, although the tests fail" }).count()) === 1 && (await page.getByRole("button", { name: /^Apply( the change| this patch)?$/ }).count()) === 0, "and offers only \"Apply anyway, although the tests fail\", never a plain Apply");
await shot("4-failing-patch");
await page.unroute("**/api/code");
await page.unroute("**/api/code/code-1");

// ---- T7: an open "almost the same" panel follows notes added on the same screen ----
await page.goto(origin + "/#/memory");
await page.getByText("Notes that say almost the same thing").first().click();
await page.getByText("No notes say almost the same thing.").waitFor({ timeout: 10000 });
for (const text of ["The backup of the customer database runs every night at the office", "The backup of the customer database runs evry night at the office"]) {
  await page.getByLabel("What should be remembered").fill(text);
  await page.getByRole("button", { name: "Remember", exact: true }).click();
  await page.getByText("Remembered.").first().waitFor({ timeout: 5000 }).catch(() => undefined);
}
await page.getByText("2 notes say almost the same thing").waitFor({ timeout: 10000 }).catch(() => undefined);
check(await page.getByText("2 notes say almost the same thing").isVisible(), "the open panel shows the two look-alike notes as soon as they are remembered, without a reload");
await shot("5-similar-refresh");

// ---- T6: a project question still being answered is picked up again when its screen is opened ----
const asking = { id: "question-1", folder: join(work, "root"), project: null, question: "Where is the discount rounded?", depth: "normal", created: 1, state: "running", failure: "", progress: ["reading pricing.py", "checking the cited lines"] };
await page.route("**/api/questions", (route) => route.request().method() === "GET" ? route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ questions: [{ id: "question-1", question: asking.question, project: null, folder: asking.folder, state: "running", created: 1, checked: null, unchecked: null }], max: 50 }) }) : route.continue());
await page.route("**/api/questions/question-1", (route) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(asking) }));
await page.goto(origin + "/#/explore");
await page.getByText("checking the cited lines").waitFor({ timeout: 10000 }).catch(() => undefined);
check(await page.getByText("checking the cited lines").isVisible() && await page.getByText("Where is the discount rounded?").first().isVisible(), "the question still being answered is shown with its progress, not refused as 'already being answered'");
await shot("6-question-picked-up");
await page.unroute("**/api/questions");
await page.unroute("**/api/questions/question-1");

check(problems.length === 0, "no console errors, page errors or failed requests: " + problems.join(" | "));
await browser.close();
server.kill();
process.exit(process.exitCode ?? 0);
