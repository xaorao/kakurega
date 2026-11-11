// Meeting notes as a person uses them: choose a transcript file, write the notes, read the checked points with their lines
// and the one the transcript does not back, keep the notes as a private note, save them as a file.
//   node tests/e2e/meeting_web.mjs      screenshots in target/meeting-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/meeting-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 19000 + (process.pid % 1000);
const procs = [];
process.on("exit", () => { for (const p of procs) { try { p.kill(); } catch {} } });
procs.push(spawn("python3", [join(repo, "tests/e2e/team_script.py"), String(port)], { stdio: "ignore" }));
await new Promise((r) => setTimeout(r, 600));
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", "team-script", "--ollama", `http://127.0.0.1:${port}`, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
procs.push(server);
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);

// a subtitle file: counters and timings between the spoken lines, and one line that holds a key
const key = ["sk-ant", "api03-abcdefghijklmnopqrstuvwxyz0123456789"].join("-");
const spoken = [
  "Asha Rao: Then we ship the new pricing page on Friday. Everyone agreed?",
  "Cara Lind: Agreed.",
  `Ben Okafor: the deploy key is ${key}`,
  "Cara Lind: I'll send the addendum to their counsel at legal@meridian.example by Wednesday.",
  "Asha Rao: Who owns the refund policy? We still have no owner for it.",
];
const vtt = "WEBVTT\n\n" + spoken.map((l, i) => `${i + 1}\n00:00:0${i}.000 --> 00:00:0${i + 1}.000\n${l}\n`).join("\n");
const file = join(work, "Weekly sync.vtt");
writeFileSync(file, vtt);

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1200 }, acceptDownloads: true });
const page = await ctx.newPage();
const errors = [];
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
page.on("pageerror", (e) => errors.push(String(e)));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.getByRole("link", { name: "Meeting notes" }).click();
await page.waitForSelector("#meet-text");
check(await page.getByRole("button", { name: "Write the notes" }).isDisabled(), "nothing can be written from an empty box");
await page.setInputFiles("#meet-file", file);
await page.waitForFunction(() => document.querySelector("#meet-text").value.length > 40);
const loaded = await page.inputValue("#meet-text");
check(!/-->|WEBVTT/.test(loaded) && loaded.split("\n").filter((l) => l.trim()).length === 5, "a subtitle file is loaded as its five spoken lines, without counters or timings");
check((await page.inputValue("#meet-title")) === "Weekly sync", "the meeting is named after the file");
await page.screenshot({ path: join(work, "1-loaded.png"), fullPage: true });
await page.getByRole("button", { name: "Write the notes" }).click();
await page.waitForSelector("#meet-result");
const result = await page.locator("#meet-result").innerText();
check(result.includes("Ship the new pricing page on Friday") && /Said on line 1:/.test(result), "the decision is shown with the line it was said on");
check(/Cara Lind: ?Send the addendum to counsel/.test(result) && result.includes("due: by Wednesday"), "the action has its owner and when");
// headings may be shown in capitals, so the split ignores case
const [facts, guesses] = result.split(/not backed by the transcript/i);
check(guesses !== undefined && guesses.includes("Hire two engineers") && !facts.includes("Hire two engineers"), "the invented action is listed apart as a guess, never among the checked points");
check(result.includes("1 line that holds a secret was not sent") && !result.includes("sk-ant"), "the line with a key was not sent and is not shown");
check(result.includes("runs on this computer"), "it says where the model that read it runs");
check(result.includes("It does not mean the point reads them correctly"), "the limit of a checked quote is stated");
await page.screenshot({ path: join(work, "2-notes.png"), fullPage: true });

await page.getByRole("button", { name: "Keep as a confidential note" }).click();
await page.getByText("Kept as a confidential note.").waitFor();
const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("button", { name: "Save as a file" }).click()]);
const saved = join(work, "saved.md");
await download.saveAs(saved);
const md = readFileSync(saved, "utf8");
check(download.suggestedFilename() === "Weekly sync.md" && md.startsWith("# Weekly sync") && md.includes("[line 1]"), "the saved file is the notes as text, named after the meeting");
await page.getByRole("link", { name: "Memory" }).click();
await page.getByText("Ship the new pricing page on Friday").first().waitFor();
check(true, "the notes are in Memory as a note");

// narrow screen and the dark theme
await page.setViewportSize({ width: 390, height: 900 });
await page.goto(new URL(link).origin + "/#/meeting");
await page.waitForSelector("#meet-text");
const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
check(!overflow, "the screen fits a phone-width window without sideways scrolling");
await page.emulateMedia({ colorScheme: "dark" });
await page.waitForTimeout(500);
await page.screenshot({ path: join(work, "3-narrow-dark.png"), fullPage: true });
check(errors.length === 0, "no console errors" + (errors.length ? ": " + errors.join(" | ") : ""));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
