// More screens whose data cannot be read say so and offer to try again, instead of saying "Loading" for ever or showing an
// empty list: each request is made to fail once, the alert is read, what an empty answer would say is checked to be absent,
// then the request is allowed and "Try again" must load it. The check page is given a file that is not a capsule.
//   node tests/e2e/states2_web.mjs      screenshots in target/states2-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/states2-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root/project-a"), { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 28000 + (process.pid % 1000);
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
const origin = new URL(link).origin;

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1000 } })).newPage();
const errors = [];
page.on("console", (m) => { if (m.type() === "error" && !/status of 500/.test(m.text())) errors.push(m.text()); });
page.on("pageerror", (e) => errors.push(String(e)));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.getByRole("link", { name: "Chat", exact: true }).click();
await page.waitForSelector("#chat-input");

const open = (summary) => async () => { await page.getByText(summary, { exact: true }).click(); };
// [what, screen, request path, text an empty answer would show (must not be shown for a failure), step before looking]
const cases = [
  ["Activity: the signed log", "#/activity", "/api/log", "Nothing recorded yet."],
  ["Activity: what is working now", "#/activity", "/api/work", "Nothing is working right now."],
  ["Activity: prices entered", "#/activity", "/api/usage/prices", "No price entered. A NEAR AI Cloud model is costed at NEAR's published price; other routes show no cost.", open("Prices you entered")],
  ["Instructions", "#/instructions", "/api/instructions", null],
  ["Instructions: the per-project switches", "#/instructions", "/api/project-settings", null],
  ["Code task: earlier tasks", "#/code", "/api/code", null],
  ["Code task: the folder listing", "#/code", "/api/fs", "No sub-folders here."],
  ["Documents: the folder listing", "#/docs", "/api/fs", "No sub-folders here."],
  ["Documents: the meaning index", "#/docs", "/api/documents/index", null],
  ["New review: the folder listing", "#/", "/api/fs", "No sub-folders here."],
  ["New review: repeating reviews", "#/", "/api/schedules", "No repeating reviews.", open("Repeat this review automatically")],
  ["Roundtable: the experts", "#/roundtable", "/api/experts", "The list of experts could not be loaded."],
  ["Team: the experts", "#/team", "/api/experts", null],
  ["Team: earlier runs", "#/team", "/api/team", "No runs yet."],
];

for (const [name, hash, path, empty, before] of cases) {
  let failing = true;
  const match = (url) => url.pathname === path;
  const handler = (route) => (failing ? route.fulfill({ status: 500, contentType: "application/json", body: "{}" }) : route.continue());
  await page.route(match, handler);
  await page.goto(origin + "/#/chat");
  await page.waitForSelector("#chat-input");
  await page.goto(origin + "/" + hash);
  if (before) await before();
  const alert = page.getByRole("alert").filter({ hasText: "could not be read from the service" });
  let shown = true;
  await alert.first().waitFor({ timeout: 8000 }).catch(() => { shown = false; });
  check(shown, `${name}: a failed load is said in words`);
  if (!shown) {
    await page.screenshot({ path: join(work, `FAILED-${name.replace(/\W+/g, "-")}.png`), fullPage: true });
    await page.unroute(match, handler);
    continue;
  }
  if (empty) check((await page.getByText(empty, { exact: true }).count()) === 0, `${name}: the failure is not shown as "${empty}"`);
  await page.screenshot({ path: join(work, `${name.replace(/\W+/g, "-")}.png`), fullPage: true });
  failing = false;
  await alert.first().getByRole("button", { name: "Try again" }).click();
  let gone = true;
  await page.getByText("could not be read from the service").first().waitFor({ state: "detached", timeout: 8000 }).catch(() => { gone = false; });
  check(gone, `${name}: trying again loads it`);
  await page.unroute(match, handler);
}

// the check page: a file that is not a capsule is refused in words, and nothing is shown as checked
await page.goto(origin + "/verify");
await page.locator("#file").waitFor();
await page.setInputFiles("#file", { name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("just some notes, not a capsule\n") });
let said = true;
await page.getByText("Not valid", { exact: true }).waitFor({ timeout: 8000 }).catch(() => { said = false; });
const body = await page.locator("main").innerText();
check(said && /not a capsule/.test(body) && /capsule_not_json/.test(body) && !/^Valid$/m.test(body), "Verify: a file that is not a capsule is refused in words");
await page.setInputFiles("#file", { name: "empty.kkg", mimeType: "application/octet-stream", buffer: Buffer.alloc(0) });
await page.getByText("capsule_not_json").waitFor({ timeout: 8000 }).catch(() => {});
check(/not a capsule/.test(await page.locator("main").innerText()), "Verify: an empty file is refused in words too");
await page.screenshot({ path: join(work, "verify-not-capsule.png"), fullPage: true });

check(errors.length === 0, "no page errors: " + errors.join(" | "));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
