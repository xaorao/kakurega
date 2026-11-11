// The roundtable as a person uses it: choose experts, ask, read each answer and the resolution, stop part-way.
// The first part runs against the real service with the scripted test model; the second shows the four-part resolution and the
// failure states from recorded stream lines of the shape the service tests produce.
//
//   node tests/e2e/roundtable_web.mjs [MODEL]     with MODEL, the first part uses that real local model (needs Ollama)
//   screenshots in target/roundtable-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/roundtable-web");
const model = process.argv[2];
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const how = model ? ["--model", model] : ["--scripted", join(repo, "tests/e2e/script.json")];
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, ...how, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
process.on("exit", () => { try { server.kill(); } catch {} });
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1100 } });
const page = await ctx.newPage();
const problems = [];
// the browser logs the deliberate 400 below as a console error; anything else counts
page.on("console", (m) => { if (m.type() === "error" && !/status of 400/.test(m.text())) problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => { if (!/ERR_ABORTED/.test(r.failure()?.errorText ?? "")) problems.push("requestfailed: " + r.url()); });
const shot = (p, n) => p.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const origin = new URL(link).origin;

await page.goto(link);
await page.waitForSelector("nav");
await page.getByRole("link", { name: "Roundtable" }).click();
await page.getByRole("heading", { name: "Roundtable", exact: true }).waitFor();
const ask = page.getByRole("button", { name: "Ask the roundtable" });
await page.getByLabel("Security reviewer").waitFor();
check((await page.locator(".rt-expert").count()) >= 10 && (await page.locator(".rt-group h2").count()) >= 4, "the experts are listed in their groups, each with what it does");
check(await ask.isDisabled(), "nothing can be asked before experts and a question are chosen");
await page.getByLabel("Security reviewer").check();
await page.getByLabel("Your question").fill("Should the payment key live in the code or in a vault?");
check(await ask.isDisabled() && /1 chosen/.test(await page.locator(".rt-experts").innerText()), "one expert is not a roundtable");
await page.getByLabel("Sceptical tester").check();
await page.getByLabel("Software architect").check();
for (const name of ["Product manager", "UX designer"]) await page.getByLabel(name).check();
check(await page.getByLabel("Data analyst").isDisabled() && /5 chosen/.test(await page.locator(".rt-experts").innerText()), "a sixth expert cannot be added");
for (const name of ["Product manager", "UX designer"]) await page.getByLabel(name).uncheck();
await shot(page, "1-chosen");
await ask.click();
await page.getByRole("heading", { name: "Resolution" }).waitFor({ timeout: 600000 });
const cards = page.locator("main ul.plain > li.card");
check((await cards.count()) === 3 && (await page.getByText("Answered", { exact: true }).count()) === 3, "each chosen expert has its own card and answered");
const first = await cards.nth(0).innerText();
check(/Security reviewer/.test(first) && first.length > 40, "the first card is the first expert chosen, with its answer");
const res = await page.locator(".rt-resolution").innerText();
check(model ? res.length > 60 : /did not use the four parts/.test(res) && /\[test model\]/.test(res), model ? "the lead wrote a resolution" : "a lead that does not use the four parts is shown as written, and said to be");
check(/advice, not as a check/.test(res), "the resolution says it is advice from the same model");
check(/not saved/.test(await page.locator("form").innerText()), "the page says a roundtable asked here is not saved");
await shot(page, "2-answered");
await page.reload();
await page.getByLabel("Security reviewer").waitFor();
check(await page.getByLabel("Security reviewer").isChecked() && await page.getByLabel("Software architect").isChecked() && !(await page.getByLabel("UX designer").isChecked()), "the choice of experts is remembered on this browser");
const fresh = await (await browser.newContext()).newPage();
const anon = await fresh.request.post(origin + "/api/chat/roundtable", { data: { message: "q", experts: ["security-reviewer", "data-analyst"] }, headers: { Origin: origin } });
check(anon.status() === 401, `a browser that is not paired cannot ask (status ${anon.status()})`);

// ---- the four-part resolution, a missing expert, and a long answer, from recorded stream lines ----
const lines = (list) => list.map((v) => JSON.stringify(v)).join("\n") + "\n";
let script = lines([
  { expert: "security-reviewer", name: "Security reviewer", state: "asking" },
  { expert: "security-reviewer", delta: "Use a " },
  { expert: "security-reviewer", answer: "Use a **vault**. Never commit the key.\n\n- rotate it\n- audit reads", truncated_for_lead: false },
  { expert: "sceptical-tester", name: "Sceptical tester", state: "asking" },
  { expert: "sceptical-tester", failed: "the model is not running" },
  { expert: "software-architect", name: "Software architect", state: "asking" },
  { expert: "software-architect", answer: "A long answer.\n\n<script>window.pwned = 1</script>\n\n[a link](javascript:alert(1))", truncated_for_lead: true },
  { resolution: { structured: true, agreed: "Keep the key out of the code.", disputed: "The architect wants a service; the reviewer a vault.", decision: "Use the vault now.", open_questions: "The tester did not answer.", text: "…" }, answered: ["security-reviewer", "software-architect"], missing: ["sceptical-tester"] },
  { done: true, reply: "Roundtable" },
]);
await page.route("**/api/chat/roundtable", (route) => route.fulfill({ status: 200, contentType: "application/x-ndjson", body: script }));
await page.getByLabel("Your question").fill("Where should the key live?");
await page.getByRole("button", { name: "Ask the roundtable" }).click();
await page.getByText("Use the vault now.").waitFor();
const all = await page.locator("main").innerText();
check(/Decision\s+Use the vault now\./.test(all) && /Agreed\s+Keep the key out of the code\./.test(all) && /Disputed/.test(all) && /Open questions\s+The tester did not answer\./.test(all), "the resolution is shown in its four parts, the decision first");
check((await page.getByText("No answer", { exact: true }).count()) === 1 && /the model is not running/.test(all), "an expert that failed says so on its own card, and the others still stand");
check(/the lead read its beginning and its end/.test(all), "a long answer says the lead read only part of it");
check((await page.evaluate(() => window.pwned)) === undefined && (await page.locator('a[href^="javascript:"]').count()) === 0, "an expert's answer cannot run script or carry a script link");
check((await page.locator(".rt-resolution strong, li.card strong").count()) >= 4, "answers are formatted (bold, lists), not shown as raw marks");
await shot(page, "3-four-parts");

script = lines([
  { expert: "security-reviewer", name: "Security reviewer", state: "asking" },
  { expert: "security-reviewer", failed: "the model is not running" },
  { expert: "sceptical-tester", name: "Sceptical tester", state: "asking" },
  { expert: "sceptical-tester", failed: "the model is not running" },
  { expert: "software-architect", name: "Software architect", state: "asking" },
  { expert: "software-architect", failed: "the model is not running" },
  { resolution: null, reason: "no expert answered, so there is nothing to resolve" },
  { error: { code: "NO_MODEL", message: "no expert could be asked" } },
]);
await page.getByRole("button", { name: "Ask the roundtable" }).click();
await page.getByRole("alert").filter({ hasText: "no expert could be asked" }).waitFor();
check((await page.getByText("No answer", { exact: true }).count()) === 3 && /nothing to resolve/.test(await page.locator("main").innerText()), "when nobody answers the page says so and shows no decision");
await shot(page, "4-nobody");
await page.unroute("**/api/chat/roundtable");
await page.route("**/api/chat/roundtable", (route) => route.fulfill({ status: 400, contentType: "application/json", body: JSON.stringify({ error: { code: "BAD_MESSAGE", message: "write a message of 1 to 4000 characters without control characters" } }) }));
await page.getByRole("button", { name: "Ask the roundtable" }).click();
await page.getByRole("alert").filter({ hasText: "1 to 4000 characters" }).waitFor();
check(true, "a refused question shows the reason in words");
await page.unroute("**/api/chat/roundtable");

check(problems.length === 0, "no console errors or failed requests: " + problems.join("; "));

const narrow = await ctx.newPage();
await narrow.setViewportSize({ width: 375, height: 800 });
await narrow.emulateMedia({ colorScheme: "dark" });
await narrow.goto(origin + "/#/roundtable");
await narrow.getByRole("heading", { name: "Roundtable", exact: true }).waitFor();
check(!(await narrow.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)), "the screen fits a phone in dark mode with no sideways scroll");
await shot(narrow, "5-phone-dark");
await browser.close();
server.kill();
