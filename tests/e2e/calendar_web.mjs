// The calendar attached in the browser as a person does it, with a stand-in for the macOS reader (the real one needs a person
// to answer macOS once; see docs/TESTING.md). The button shows only where a reader exists; loading the page never asks macOS;
// pressing it asks once, attaches the next seven days like a file, and a key in an event's notes is held back.
//   node tests/e2e/calendar_web.mjs      screenshots in target/calendar-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/calendar-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
// the stand-in keeps what macOS "allows" in a file, records each time it is asked, and answers like kaku-calendar
const state = join(work, "allowed");
const asked = join(work, "asked");
writeFileSync(state, "not_asked");
const reader = join(work, "kaku-calendar");
const key = ["sk", "ant", "api03-calendarnotes0123456789abcdefghijklmnopqrstuvwxyzABCDEFGH"].join("-");
writeFileSync(reader, `#!/bin/sh
s=$(cat ${state})
case "$1" in
  status) echo "{\\"calendar\\":\\"$s\\",\\"reminders\\":\\"$s\\"}";;
  ask) echo x >> ${asked}; echo allowed > ${state}; echo '{"calendar":"allowed","reminders":"allowed"}';;
  events) echo '{"events":[{"title":"Board call","start":"2026-11-14T09:00:00Z","end":"2026-11-14T09:30:00Z","all_day":false,"location":"Room 4","calendar":"Work","notes":"the deploy key is ${key}"}],"truncated":false}';;
  reminders) echo '{"reminders":[{"title":"Pay the invoice","due":"2026-11-16T17:00:00Z","list":"Personal","notes":""}],"truncated":false}';;
esac
`);
chmodSync(reader, 0o755);
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 27000 + (process.pid % 1000);
const procs = [];
process.on("exit", () => { for (const p of procs) { try { p.kill(); } catch {} } });
procs.push(spawn("python3", [join(repo, "tests/e2e/team_script.py"), String(port)], { stdio: "ignore" }));
await new Promise((r) => setTimeout(r, 600));
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", "team-script", "--ollama", `http://127.0.0.1:${port}`, "--ui-dir", join(repo, "apps/web/dist")], {
  stdio: ["ignore", "pipe", "pipe"],
  env: { ...process.env, KAKU_CALENDAR: reader, KAKU_CALENDAR_UNCONFINED: "1" },
});
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
page.on("console", (m) => { if (m.type() === "error" && !/409|Conflict/.test(m.text())) errors.push(m.text()); });
page.on("pageerror", (e) => errors.push(String(e)));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const timesAsked = () => (existsSync(asked) ? readFileSync(asked, "utf8").trim().split("\n").filter(Boolean).length : 0);

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.getByRole("link", { name: "Chat", exact: true }).click();
await page.waitForSelector("#chat-input");
const button = page.getByRole("button", { name: "Attach my calendar" });
await button.waitFor();
check(timesAsked() === 0, "opening the chat shows the button and does not ask macOS anything");
check(/The first time, macOS asks/.test(await page.locator("#calendar-help").innerText()), "the button says macOS will ask the first time");
await button.click();
await page.getByText("calendar-next-7-days.md").waitFor();
check(timesAsked() === 1, "pressing it asks macOS once");
const chip = page.locator(".file-chip", { hasText: "calendar-next-7-days.md" });
check(/what looks like a secret/.test(await chip.innerText()), "a key in an event's notes is flagged and will not be given to the model");
await chip.getByRole("button", { name: "What was read" }).click();
await chip.getByText("Board call").first().waitFor();
const read = await chip.innerText();
check(/Board call \(Work\), at Room 4/.test(read) && /Pay the invoice/.test(read), "what was read is the week's events and the open reminders, shown before anything is sent");
await page.screenshot({ path: join(work, "1-attached.png"), fullPage: true });
await button.click();
await page.waitForFunction(() => document.querySelectorAll(".file-chip").length >= 1);
await page.waitForTimeout(500);
check((await page.locator(".file-chip", { hasText: "calendar-next-7-days.md" }).count()) === 1 && timesAsked() === 1, "pressing it again replaces the same attachment and does not ask again");

// macOS refused: said in words, nothing attached
writeFileSync(state, "denied");
await chip.getByRole("button", { name: "Remove" }).click();
await button.click();
await page.getByText("System Settings").first().waitFor();
check(/does not allow Kakurega to read the calendar/.test(await page.locator(".attach").innerText()) && (await page.locator(".file-chip").count()) === 0, "a refusal says where to allow it and attaches nothing");
await page.screenshot({ path: join(work, "2-refused.png"), fullPage: true });

// where there is no reader the button is not offered
const bare = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data2"), "--root", root, "--model", "team-script", "--ollama", `http://127.0.0.1:${port}`, "--ui-dir", join(repo, "apps/web/dist")], {
  stdio: ["ignore", "pipe", "pipe"],
  env: { ...process.env, KAKU_CALENDAR: join(work, "no-such-reader") },
});
procs.push(bare);
let log2 = "";
bare.stdout.on("data", (d) => (log2 += d));
bare.stderr.on("data", (d) => (log2 += d));
const d2 = Date.now() + 30000;
while (!/#pair=/.test(log2) && Date.now() < d2) await new Promise((r) => setTimeout(r, 100));
const link2 = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log2)?.[0];
const p2 = await (await browser.newContext({ viewport: { width: 1280, height: 1000 } })).newPage();
await p2.goto(link2);
await p2.waitForSelector("nav");
const skip2 = p2.getByRole("button", { name: "Skip the rest" });
if (await skip2.count()) await skip2.click();
await p2.getByRole("link", { name: "Chat", exact: true }).click();
await p2.waitForSelector("#chat-input");
await p2.waitForTimeout(800);
check((await p2.getByRole("button", { name: "Attach my calendar" }).count()) === 0, "without a reader (not a Mac) there is no button");
check(errors.length === 0, `no console errors: ${errors.join(" | ")}`);
await browser.close();
console.log(process.exitCode ? "calendar checks failed" : "all calendar checks passed");
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
