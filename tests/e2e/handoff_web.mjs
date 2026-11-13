// Handing text to the Claude Code installed on this computer, as a person does it: write, read back exactly what will go,
// send, read the answer. It runs the real program once with a one-line question, so it needs Claude Code installed and
// signed in; without it the test checks that the screen says so.
//   node tests/e2e/handoff_web.mjs      screenshots in target/handoff-web/

import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/handoff-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const installed = spawnSync("claude", ["--version"], { encoding: "utf8" }).status === 0;
const port = 25000 + (process.pid % 1000);
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
await page.locator("#nav-more:not([open]) > summary").click({ timeout: 3000 }).catch(() => {}); // these links sit under More
await page.getByRole("link", { name: "Ask Claude Code" }).click();
if (!installed) {
  await page.waitForSelector("#hand-none");
  check(true, "with no coding agent installed the screen says so and offers nothing to send");
  console.log("SKIP the rest: Claude Code is not installed here");
} else {
  await page.waitForSelector("#hand-text");
  check((await page.locator("#hand-notice").innerText()).startsWith("This is not private."), "the screen says first that this is not private");
  check(await page.getByRole("button", { name: "Show me exactly what will be sent" }).isDisabled(), "nothing can be shown from an empty box");
  await page.fill("#hand-text", "the staging password is hunter2, why does login fail?");
  await page.getByRole("button", { name: "Show me exactly what will be sent" }).click();
  await page.getByRole("alert").waitFor();
  check((await page.getByRole("alert").innerText()).includes("key or a password"), "text with a password in it is refused before anything is shown or sent");
  const question = "Reply with exactly these two words and nothing else: kakurega handoff\nMy address is asha@meridian.example";
  await page.fill("#hand-text", question);
  await page.getByRole("button", { name: "Show me exactly what will be sent" }).click();
  await page.waitForSelector("#hand-confirm");
  check((await page.locator("#hand-exact").innerText()) === question, "the exact text is shown back");
  const confirm = await page.locator("#hand-confirm").innerText();
  check(confirm.includes("1 email address") && /[0-9a-f]{64}/.test(confirm), "it says the text holds an email address and gives the text's fingerprint");
  await page.screenshot({ path: join(work, "1-confirm.png"), fullPage: true });
  await page.getByRole("button", { name: "Do not send" }).click();
  await page.waitForSelector("#hand-text");
  check((await page.inputValue("#hand-text")) === question && (await page.locator("#hand-reply").count()) === 0, "saying no sends nothing and keeps what was written");
  // the real send costs a call on the owner's own account, so it is made only when asked for: KK_HANDOFF_REAL=1
  if (process.env.KK_HANDOFF_REAL !== "1") {
    console.log("SKIP the real send: set KK_HANDOFF_REAL=1 to hand one line to the real Claude Code");
  } else {
  await page.fill("#hand-text", "Reply with exactly these two words and nothing else: kakurega handoff");
  await page.getByRole("button", { name: "Show me exactly what will be sent" }).click();
  await page.waitForSelector("#hand-confirm");
  await page.getByRole("button", { name: /^Send it to Claude Code/ }).click();
  await page.waitForSelector("#hand-reply", { timeout: 240000 });
  const reply = await page.locator("#hand-reply").innerText();
  check(/kakurega handoff/i.test(reply), "the real Claude Code answered the text it was handed");
  check(reply.includes("not from your private model"), "the answer is labelled as an outside opinion");
  await page.screenshot({ path: join(work, "2-reply.png"), fullPage: true });
  await page.getByRole("link", { name: "Where your data goes" }).click();
  await page.waitForSelector("#outbound li.out-row");
  check((await page.locator("#outbound").innerText()).includes("Claude Code"), "the hand-over is listed among what left this computer");
  }
}
const other = errors.filter((e) => !/status of 422/.test(e));
check(other.length === 0, "no console errors beyond the one refusal" + (other.length ? ": " + other.join(" | ") : ""));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
