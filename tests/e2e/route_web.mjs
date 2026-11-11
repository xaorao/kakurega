// Choosing where the model runs, as a person does it in Settings: local model first, then NEAR AI Cloud with a model
// name and an account key, then back. The key here is made up, so nothing can be answered by NEAR; what is checked is the
// switch itself, where the key ends up, what is shown, and that the choice is kept for the next start.
//   node tests/e2e/route_web.mjs      screenshots in target/route-web/

import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/route-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
const data = join(work, "data");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 23000 + (process.pid % 1000);
const procs = [];
process.on("exit", () => { for (const p of procs) { try { p.kill(); } catch {} } });
procs.push(spawn("python3", [join(repo, "tests/e2e/team_script.py"), String(port)], { stdio: "ignore" }));
await new Promise((r) => setTimeout(r, 600));
const KEY = ["sk", "test", "route", "0123456789abcdefghij"].join("-");

async function serve() {
  const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", data, "--root", join(work, "root"), "--model", "team-script", "--ollama", `http://127.0.0.1:${port}`, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
  procs.push(server);
  let log = "";
  server.stdout.on("data", (d) => (log += d));
  server.stderr.on("data", (d) => (log += d));
  const deadline = Date.now() + 30000;
  while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
  const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
  if (!link) throw new Error("no pairing link:\n" + log);
  return { server, link, log: () => log };
}
const filesHolding = (dir, needle) => {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...filesHolding(p, needle));
    else if (readFileSync(p).includes(needle)) out.push(p.slice(data.length + 1));
  }
  return out;
};

let { server, link, log } = await serve();
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1200 } });
let page = await ctx.newPage();
const errors = [];
const watch = (p) => { p.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); }); p.on("pageerror", (e) => errors.push(String(e))); };
watch(page);
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const open = async (l) => {
  await page.goto(l);
  await page.waitForSelector("nav");
  const skip = page.getByRole("button", { name: "Skip the rest" });
  if (await skip.count()) await skip.click();
  await page.goto(new URL(l).origin + "/#/settings");
  await page.waitForSelector("#route-current");
};

await open(link);
check((await page.locator("#route-current").innerText()).includes("the model team-script on this computer"), "it starts on the model on this computer");
check(await page.getByRole("button", { name: "Save and use NEAR AI Cloud" }).isDisabled(), "NEAR cannot be chosen without a model and a key");
await page.fill("#route-model", "deepseek-ai/DeepSeek-V3.1");
await page.fill("#route-key", "has spaces in it");
await page.getByRole("button", { name: "Save and use NEAR AI Cloud" }).click();
await page.locator("#route-choice").getByRole("alert").waitFor();
check((await page.locator("#route-choice").getByRole("alert").innerText()).includes("not a plain account key") && (await page.locator("#route-current").innerText()).includes("on this computer"), "a key that is not a key is refused in words and nothing changes");
await page.fill("#route-key", KEY);
await page.getByRole("button", { name: "Save and use NEAR AI Cloud" }).click();
await page.getByText("NEAR AI Cloud answers from now on").waitFor();
const now = await page.locator("#route-current").innerText();
check(now.includes("deepseek-ai/DeepSeek-V3.1 on NEAR AI Cloud") && now.includes("near.ai"), "the route in use is NEAR, with its model and host");
check((await page.inputValue("#route-key")) === "", "the key box is empty again");
await page.waitForFunction(() => !document.querySelector("header")?.textContent?.includes("model runs on this computer"));
check(!(await page.locator("header").innerText()).includes("model runs on this computer"), "the header no longer says the model runs on this computer");
check(!(await page.content()).includes(KEY), "the key is nowhere in the page");
await page.getByText("In use now: the model that answers is not on this computer.").waitFor();
check(true, "the names-to-hide setting on the same page now says it is in use");
const box = await page.evaluate(() => { const k = document.querySelector("#route-key").getBoundingClientRect(); const m = document.querySelector("#route-model").getBoundingClientRect(); return [k.height, m.height]; });
check(Math.abs(box[0] - box[1]) < 2, "the key box looks like the other boxes");
await page.screenshot({ path: join(work, "1-near.png"), fullPage: true });

const holding = filesHolding(data, KEY);
check(holding.length === 1 && holding[0] === "near.key", "on disk the key is in one file only: " + holding.join(", "));
check((statSync(join(data, "near.key")).mode & 0o777) === 0o600, "that file can be read by its owner only");
check(!log().includes(KEY), "the key is not in the service's output");

// the choice is kept: a service started again with no flag about the route is on NEAR
await page.close(); // its live connection would report the service going away, which is the point of the restart
server.kill();
await new Promise((r) => setTimeout(r, 500));
({ server, link, log } = await serve());
page = await ctx.newPage();
watch(page);
await open(link);
check((await page.locator("#route-current").innerText()).includes("on NEAR AI Cloud"), "after a restart the saved choice is in use");
check(await page.getByRole("button", { name: "Forget the saved key" }).count() === 0, "the key cannot be forgotten while it is in use");

// back to this computer, and the model here answers
await page.getByRole("button", { name: "Use the model on this computer" }).click();
await page.getByText("The model on this computer answers from now on.").waitFor();
check((await page.locator("#route-current").innerText()).includes("on this computer"), "switched back to the model on this computer");
const reply = await page.evaluate(async () => {
  const csrf = (await (await fetch("/api/session", { credentials: "same-origin" })).json()).csrf;
  const r = await fetch("/api/chat", { method: "POST", headers: { "Content-Type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify({ message: "hello" }) });
  return { status: r.status, body: await r.json() };
});
check(reply.status === 200 && reply.body.route.stays_here === true, "a question is answered by the model on this computer");
await page.getByRole("button", { name: "Forget the saved key" }).click();
await page.getByText("The saved key was removed from this computer.").waitFor();
check(!existsSync(join(data, "near.key")) && filesHolding(data, KEY).length === 0, "the key is gone from this computer");
check(await page.getByRole("button", { name: "Save and use NEAR AI Cloud" }).isDisabled(), "NEAR needs the key again");
await page.screenshot({ path: join(work, "2-local.png"), fullPage: true });
const other = errors.filter((e) => !/status of 422/.test(e));
check(other.length === 0, "no console errors beyond the one refusal" + (other.length ? ": " + other.join(" | ") : ""));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
