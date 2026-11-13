// A tool set to "ask me each time", with a real local model: a coding task tries to call it, the call waits, the owner sees the
// exact arguments on the Code page and approves, and the task gets the tool's answer; a second call is denied and refused.
// Needs Ollama with the model (default qwen2.5-coder:7b), `cargo build -p kk-cli -p kk-mcpc --bins`.
//
//   node tests/e2e/mcp_ask_web.mjs [MODEL]     screenshots in target/mcp-ask-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/mcp-ask-web");
const model = process.argv[2] ?? "qwen2.5-coder:7b";
// KAKU_TEST_NEAR_MODEL and KAKU_TEST_NEAR_KEY_FILE run the model work on NEAR AI Cloud's confidential hardware instead
const nearModel = process.env.KAKU_TEST_NEAR_MODEL;
const near = nearModel ? ["--route", "near", "--near-model", nearModel, "--near-key-file", process.env.KAKU_TEST_NEAR_KEY_FILE ?? ""] : null;

rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const proj = join(root, "proj");
mkdirSync(join(proj, "src"), { recursive: true });
writeFileSync(join(proj, "src/app.py"), "def add(a, b):\n    return a - b\n");
const fake = join(repo, "target/debug/fake_mcp");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", model, ...(near ?? []), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
process.on("exit", () => { try { server.kill(); } catch {} });
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1000 } })).newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { if (!cond) { console.log("FAIL", what); process.exitCode = 1; } else console.log("PASS", what); };
const origin = new URL(link).origin;
const api = (method, path, body) => page.evaluate(async ([method, path, body]) => {
  const csrf = (await (await fetch("/api/session", { credentials: "same-origin" })).json()).csrf;
  const r = await fetch(path, { method, headers: { "Content-Type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: body ? JSON.stringify(body) : undefined });
  return { status: r.status, body: await r.json().catch(() => ({})) };
}, [method, path, body]);

await page.goto(link);
await page.waitForSelector("nav");
await api("POST", "/api/mcp/servers", { name: "docs", program: fake, args: [], env: { FAKE_MODE: "annot" } });
await api("POST", "/api/mcp/discover", { name: "docs" });
const tools = (await api("GET", "/api/mcp")).body.servers[0].tools;
const echo = tools.find((t) => t.name === "echo");
check((await api("POST", "/api/mcp/tools", { server: "docs", tool: "echo", mode: "ask", pin: echo.pin })).status === 200, "echo is set to ask");

const start = async (prompt) => {
  const r = await api("POST", "/api/code", { folder: proj, task: prompt });
  if (r.status !== 202) throw new Error("task not started: " + JSON.stringify(r));
  return r.body.id;
};
const finish = async (id) => {
  for (let i = 0; i < 300; i++) {
    const v = (await api("GET", "/api/code/" + id)).body;
    if (v.state && v.state !== "running") return v;
    await new Promise((r) => setTimeout(r, 2000));
  }
  throw new Error("task did not finish");
};
const ask = 'Call the tool mcp__docs__echo with the text "ping" and tell me what it answers. Then call finish.';

let id = await start(ask);
await page.goto(origin + "/#/code");
const card = page.getByRole("heading", { name: "A coding task wants to call an outside tool" });
await card.waitFor({ timeout: 120000 });
await shot("1-waiting");
const shown = await page.locator("#h-mcp-pending").locator("xpath=..").innerText();
check(/docs \/ echo with: \{.*\}/.test(shown), "the waiting call shows its tool and its exact arguments (whatever the model sent): " + shown.split("\n").find((l) => /docs \/ echo/.test(l)));
await page.getByRole("button", { name: "Approve this call" }).click();
let v = await finish(id);
await shot("2-approved");
const first = (v.steps ?? []).find((x) => x.tool === "mcp__docs__echo");
check(first && first.refused === false && /Result from an outside tool/.test(first.said), "after approval the call ran and its answer came back as untrusted text (step not refused)");

id = await start(ask);
await card.waitFor({ timeout: 120000 });
await page.getByRole("button", { name: "Deny" }).click();
v = await finish(id);
const steps = JSON.stringify(v.steps ?? []);
check(!/echo: ping/.test(JSON.stringify(v)) || /did not approve/.test(JSON.stringify(v)), "after a denial the task did not get the tool's answer");
console.log("OBSERVED denied steps:", steps.slice(0, 300));
check(problems.length === 0, "no console errors: " + problems.join("; "));
await browser.close();
server.kill();
process.exit(process.exitCode ?? 0);
