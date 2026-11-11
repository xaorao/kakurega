// A team run as a person uses it: state a goal, read the lead's plan, start it, watch tasks run, answer an expert's question,
// read the result; then a plan the lead gets wrong, a stop and a carry-on, and a delete.
// The model is a stand-in server that speaks the local model's protocol (tests/e2e/team_script.py), so the whole path from the
// page to the model route is the real one.
//
//   node tests/e2e/team_web.mjs [MODEL]     with MODEL, a real local model is used and only the main path is checked
//   screenshots in target/team-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/team-web");
const real = process.argv[2];
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 18000 + (process.pid % 1000);
const procs = [];
process.on("exit", () => { for (const p of procs) { try { p.kill(); } catch {} } });
let how;
if (real?.startsWith("anthropic:")) {
  // the outside test model: the key comes from the environment (ANTHROPIC_API_KEY), never from an argument
  how = ["--route", "outside", "--outside-model", real];
} else if (real) {
  how = ["--model", real];
} else {
  procs.push(spawn("python3", [join(repo, "tests/e2e/team_script.py"), String(port)], { stdio: "ignore" }));
  how = ["--model", "team-script", "--ollama", `http://127.0.0.1:${port}`];
  await new Promise((r) => setTimeout(r, 600));
}
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, ...how, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
procs.push(server);
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1100 } });
const page = await ctx.newPage();
const problems = [];
// the browser logs the deliberate refusals below as console errors; anything else counts
page.on("console", (m) => { if (m.type() === "error" && !/status of (409|422)/.test(m.text())) problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => { if (!/ERR_ABORTED/.test(r.failure()?.errorText ?? "")) problems.push("requestfailed: " + r.url()); });
const shot = (p, n) => p.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const long = real ? 900000 : 30000;

await page.goto(link);
await page.waitForSelector("nav");
// the first-run welcome is its own screen's business
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.getByRole("link", { name: "Team", exact: true }).click();
await page.getByRole("heading", { name: "Team", exact: true }).waitFor();
const plan = page.getByRole("button", { name: "Make a plan" });
check(await plan.isDisabled(), "a plan cannot be asked for without a goal");
check(/No runs yet/.test(await page.locator("main").innerText()), "an empty list says so");
await page.getByLabel("The goal").fill("Decide where our payment key should live and tell the board.");
await plan.click();
await page.getByRole("heading", { name: /This run/ }).waitFor({ timeout: long });
const status = () => page.locator("#team-run [data-status]").getAttribute("data-status");
const states = () => page.locator(".team-tasks > li").evaluateAll((els) => els.map((e) => e.getAttribute("data-state")));
check((await status()) === "planned" && /Not started/.test(await page.locator("#team-run").innerText()), "the plan is shown and is not started");
const planned = await states();
check(planned.length >= 1 && planned.every((s) => s === "waiting"), "every task of the plan waits: nothing ran");
if (!real) {
  const tasks = await page.locator(".team-tasks").innerText();
  check(/Task 1 · Security reviewer/.test(tasks) && /Task 3 · Plain-language writer/.test(tasks) && /builds on 1, 2/.test(tasks), "each task names its expert and what it builds on");
}
await shot(page, "1-plan");
await page.getByRole("button", { name: "Start the plan" }).click();
if (!real) {
  await page.locator('.team-tasks > li[data-task="1"][data-state="running"]').waitFor({ timeout: 10000 });
  check((await status()) === "running" && (await page.getByRole("button", { name: "Stop" }).count()) === 1 && (await page.getByRole("button", { name: "Delete" }).count()) === 0, "while it works it can be stopped and cannot be deleted");
  await shot(page, "2-working");
  // the Activity screen lists it as working, and it can be opened from there
  const side = await ctx.newPage();
  await side.goto(new URL(link).origin + "/#/activity");
  const working = side.locator("#working .work-item");
  await working.first().waitFor({ timeout: 10000 });
  check((await working.count()) === 1 && /Team run/.test(await working.first().innerText()) && /Decide where our payment key/.test(await working.first().innerText()) && (await working.first().getByRole("button", { name: "Stop" }).count()) === 1 && (await working.first().getAttribute("data-quiet")) === "no", "the Activity screen lists the run as working, with a stop, and not as gone quiet");
  await side.close();
  await page.locator("#team-run [data-status='waiting']").waitFor({ timeout: long });
  check(JSON.stringify(await states()) === JSON.stringify(["done", "needs_input", "waiting"]), "an expert's question holds back only what builds on it");
  const one = page.locator('.team-tasks > li[data-task="1"]');
  check((await one.locator("ol > li").count()) === 2, "a result is formatted (a numbered list), not shown as raw marks");
  check((await page.evaluate(() => window.pwned)) === undefined && /<script>/.test(await one.innerText()), "script text inside a result is shown as text and does not run");
  const two = page.locator('.team-tasks > li[data-task="2"]');
  check(/This expert asks: Which cloud do you deploy to\?/.test(await two.innerText()) && /Waiting for you/.test(await page.locator("#team-run").innerText()), "the question is shown where it was asked, and the run says it waits for the owner");
  check(await two.getByRole("button", { name: "Answer and carry on" }).isDisabled() && (await page.getByRole("button", { name: "Carry on", exact: true }).count()) === 0, "an empty answer cannot be sent, and the only way on is to answer");
  await shot(page, "3-asks-you");
  // the question is also in "Waiting for you", and opening it there lands on this run
  await page.getByRole("link", { name: /Waiting for you/ }).click();
  const waitingItem = page.locator("main li.card").filter({ hasText: "An expert asks you" });
  await waitingItem.waitFor();
  check(/Software architect asks: Which cloud do you deploy to\?/.test(await waitingItem.innerText()) && /Decide where our payment key/.test(await waitingItem.innerText()), "the question is listed in Waiting for you with the run's goal");
  await waitingItem.getByRole("link", { name: "Open" }).click();
  await page.locator('.team-tasks > li[data-task="2"][data-state="needs_input"]').waitFor();
  check(/#\/team\/team-[0-9a-f]{12}$/.test(page.url()), "opening it shows the run, and the address names the run");
  await page.reload();
  await page.locator('.team-tasks > li[data-task="2"][data-state="needs_input"]').waitFor();
  check(true, "a reload of that address shows the same run");
  await two.getByRole("textbox").fill("Our own cloud account in Mumbai.");
  await two.getByRole("button", { name: "Answer and carry on" }).click();
}
if (real) {
  // a real model may stop to ask: answer whatever it asks, at most six times, until the run ends
  for (let round = 0; round < 6; round++) {
    await page.locator("#team-run [data-status='done'], #team-run [data-status='waiting']").waitFor({ timeout: long });
    if ((await status()) === "done") break;
    const asking = page.locator('.team-tasks > li[data-state="needs_input"]').first();
    console.log("NOTE the model asked:", (await asking.locator("label").innerText()).slice(0, 200));
    await asking.getByRole("textbox").fill("We have no figures yet. We deploy on our own cloud account, and the key signs card payments.");
    await asking.getByRole("button", { name: "Answer and carry on" }).click();
    await page.locator("#team-run [data-status='running']").waitFor({ timeout: 15000 }).catch(() => undefined);
  }
}
await page.locator("#team-run [data-status='done']").waitFor({ timeout: long });
const done = await states();
check(real ? done.every((s) => ["done", "failed", "skipped"].includes(s)) : done.every((s) => s === "done"), "the run ends with every task ended: " + done.join(","));
const result = page.locator("section[aria-labelledby='team-final']");
check((await result.count()) === 1 && (await result.innerText()).length > 20, "the lead's result is shown above the tasks");
if (!real) {
  check((await result.locator("strong").count()) >= 1 && /Keep the payment key in the vault/.test(await result.innerText()) && /advice, not as a check/.test(await result.innerText()), "the result is formatted and says it is advice, not a check");
  check(/Asked: Which cloud do you deploy to\?\s*You answered: Our own cloud account in Mumbai\./.test(await page.locator('.team-tasks > li[data-task="2"]').innerText()), "the question and the owner's answer stay on the task");
  const first = page.locator(".team-runs li").first();
  check(/Finished/.test(await first.innerText()) && /3 of 3 done/.test(await first.innerText()), "the list of runs shows it finished, 3 of 3");
  check((await page.getByRole("button", { name: "Start the plan" }).count()) === 0 && (await page.getByRole("button", { name: "Carry on", exact: true }).count()) === 0, "a finished run offers no start");
}
await shot(page, "4-result");

if (!real) {
  // ---- a plan the lead gets wrong starts nothing and says why ----
  await page.getByLabel("The goal").fill("refuse-this-plan please");
  await plan.click();
  const alert = page.getByRole("alert");
  await alert.waitFor({ timeout: long });
  check(/did not write a usable plan/.test(await alert.innerText()) && /Nothing was started/.test(await alert.innerText()), "an unusable plan is refused in words");
  check((await page.locator(".team-runs li").count()) === 1 && (await page.getByLabel("The goal").inputValue()) === "refuse-this-plan please", "nothing was kept, and the goal is still there to change");

  // ---- the lead may be limited to chosen experts ----
  await page.getByText("Choose which experts the lead may use").click();
  await page.getByLabel("Plain-language writer").check();
  await page.getByLabel("The goal").fill("Say it plainly.");
  await plan.click();
  await alert.waitFor({ timeout: long });
  check(/task 1 names 'security-reviewer', which is not on the list/.test(await alert.innerText()), "an expert the owner did not allow is refused by name");
  await page.getByLabel("Plain-language writer").uncheck();

  // ---- stop while a task is being asked, then carry on ----
  await page.getByLabel("The goal").fill("Decide again where the key lives.");
  await plan.click();
  await page.locator("#team-run [data-status='planned']").waitFor({ timeout: long });
  await page.getByRole("button", { name: "Start the plan" }).click();
  await page.locator('.team-tasks > li[data-task="1"][data-state="running"]').waitFor({ timeout: 10000 });
  await page.getByRole("button", { name: "Stop" }).click();
  await page.locator("#team-run [data-status='stopped']").waitFor({ timeout: 15000 });
  const afterStop = await states();
  const stopCard = await page.locator("section[aria-labelledby='team-run'] > .card").first().innerText();
  check(afterStop.every((s) => s === "waiting") && /Finished tasks are kept/.test(stopCard), "a stop ends the answer being written and leaves the tasks waiting: " + afterStop.join(",") + " | " + stopCard.replace(/\n/g, " / ").slice(0, 200));
  await shot(page, "5-stopped");
  // leaving the page and coming back finds the run again from the list
  await page.getByRole("link", { name: "Roundtable" }).click();
  await page.getByRole("link", { name: "Team", exact: true }).click();
  await page.locator(".team-runs li").first().getByRole("button").click();
  await page.locator("#team-run [data-status='stopped']").waitFor();
  await page.getByRole("button", { name: "Carry on", exact: true }).click();
  await page.locator("#team-run [data-status='done']").waitFor({ timeout: long });
  check((await states()).every((s) => s === "done"), "carrying on finishes the run");
  // ---- delete asks first ----
  await page.getByRole("button", { name: "Delete" }).click();
  check(/Delete this run and its results\?/.test(await page.locator("#team-run").locator("xpath=..").innerText()), "deleting asks first");
  await page.getByRole("button", { name: "Keep it" }).click();
  check((await page.locator(".team-runs li").count()) === 2, "keeping it keeps it");
  await page.getByRole("button", { name: "Delete" }).click();
  await page.getByRole("button", { name: "Yes, delete" }).click();
  await page.getByRole("heading", { name: /This run/ }).waitFor({ state: "detached" });
  check((await page.locator(".team-runs li").count()) === 1, "after a yes the run is gone from the list");
}
if (!real) {
  await page.getByRole("link", { name: "Activity" }).click();
  await page.locator("#working").waitFor();
  await page.waitForFunction(() => /Nothing is working right now/.test(document.querySelector("#working")?.textContent ?? ""), null, { timeout: 10000 });
  check(true, "when nothing runs, the Activity screen says nothing is working");
  await shot(page, "7-activity");
}
// ---- files for the whole run ----
{
  await page.goto(new URL(link).origin + "/#/team");
  await page.getByRole("heading", { name: "Team", exact: true }).waitFor();
  // the seat that works on the project's files is offered, under its own heading, and is not ticked by default
  await page.getByText("Choose which experts the lead may use").click();
  const reviewer = page.locator("label.rt-expert", { hasText: "Project reviewer" });
  await reviewer.waitFor({ timeout: long });
  check((await page.getByRole("heading", { name: "Works on the project's files, in the sandbox" }).count()) === 1 && (await reviewer.count()) === 1 && !(await reviewer.locator("input").isChecked()), "the Project reviewer is offered under its own heading and is not chosen unless the owner ticks it");
  check((await page.locator("#team-project").count()) === 0, "with no project made, no project choice is shown");
  const made = ["sk-ant", "api03-abcdefghijklmnopqrstuvwxyz0123456789"].join("-");
  const notes = { name: "billing-notes.txt", mimeType: "text/plain", buffer: Buffer.from(`Refunds over 500 need a second approval.\nAPI_KEY=${made}\nThe export runs nightly.\n`) };
  await page.locator("#team-files").setInputFiles(notes);
  const list = page.getByRole("list", { name: "Files for this run" });
  await list.getByText("billing-notes.txt").waitFor({ timeout: long });
  check(/1 line\(s\) with a key withheld/.test(await list.innerText()), "a file attached for the run is listed, and says a line with a key is withheld");
  await list.getByRole("button", { name: "Remove" }).click();
  check((await list.count()) === 0, "it can be removed again before planning");
  await page.locator("#team-files").setInputFiles(notes);
  await list.getByText("billing-notes.txt").waitFor({ timeout: long });
  await page.getByLabel("The goal").fill("Read the billing notes and say what a new teammate must know.");
  await page.getByRole("button", { name: "Make a plan" }).click();
  await page.getByText("With the files: billing-notes.txt").waitFor({ timeout: long });
  check((await list.count()) === 0 && (await page.locator("#team-run [data-status]").getAttribute("data-status")) === "planned", "the planned run names its files and the form is cleared");
  await shot(page, "7-run-with-files");
}
check(problems.length === 0, "no console errors or failed requests: " + problems.join(" | "));

// ---- a phone in dark mode ----
const phone = await browser.newContext({ viewport: { width: 375, height: 800 }, colorScheme: "dark", storageState: await ctx.storageState() });
const pp = await phone.newPage();
await pp.goto(new URL(link).origin + "/#/team");
await pp.getByRole("heading", { name: "Team", exact: true }).waitFor();
await pp.locator(".team-runs li").first().getByRole("button").click();
await pp.getByRole("heading", { name: /This run/ }).waitFor();
check(await pp.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "the screen fits a phone in dark mode with no sideways scroll");
await shot(pp, "6-phone-dark");
await browser.close();
for (const p of procs) p.kill();
