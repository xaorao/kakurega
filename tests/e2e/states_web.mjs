// A screen whose data cannot be read says so and offers to try again, instead of saying "Loading" for ever: the service's
// answer is made to fail once for four screens, then allowed.
//   node tests/e2e/states_web.mjs

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/states-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
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

for (const [name, hash, path] of [["Waiting for you", "#/inbox", "**/api/inbox"], ["Agent access", "#/agent", "**/api/agent-access"], ["Skills (methods)", "#/skills", "**/api/methods"], ["Skills (tool servers)", "#/skills", "**/api/mcp"]]) {
  let failing = true;
  await page.route(path, (route) => (failing ? route.fulfill({ status: 500, contentType: "application/json", body: "{}" }) : route.continue()));
  await page.goto(new URL(link).origin + "/#/chat");
  await page.goto(new URL(link).origin + "/" + hash);
  const alert = page.getByRole("alert").filter({ hasText: "could not be read from the service" });
  await alert.first().waitFor();
  check(true, `${name}: a failed load is said in words`);
  failing = false;
  await alert.first().getByRole("button", { name: "Try again" }).click();
  await alert.first().waitFor({ state: "detached" });
  check((await page.getByText("could not be read from the service").count()) === 0, `${name}: trying again loads it`);
  await page.unroute(path);
}
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
