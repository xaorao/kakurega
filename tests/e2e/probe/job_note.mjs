// Prints a stored review's state, note and release answer from a copy of a data folder (helper; not part of any gate).
//   node tests/e2e/probe/job_note.mjs DATA_PARENT_DIR [JOB]      e.g. target/film-flow job-1
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { cpSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const from = resolve(process.argv[2]); const id = process.argv[3] ?? "job-1";
const work = join(repo, "target/job-note"); rmSync(work, { recursive: true, force: true });
cpSync(join(from, "data"), join(work, "data"), { recursive: true }); cpSync(join(from, "root"), join(work, "root"), { recursive: true });
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", join(work, "root"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = ""; server.stdout.on("data", (d) => (log += d)); server.stderr.on("data", (d) => (log += d));
while (!/#pair=/.test(log)) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)[0];
const browser = await chromium.launch({ executablePath: join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"), headless: true });
const page = await (await browser.newContext()).newPage();
await page.goto(link); await page.waitForSelector("nav");
const out = await page.evaluate(async (id) => {
  const j = await (await fetch(`/api/jobs/${id}`)).json(); const r = await (await fetch(`/api/jobs/${id}/release`)).json(); const f = await (await fetch(`/api/jobs/${id}/findings`)).json();
  return { state: j.state, note: j.note, events: j.events.map((e) => e.stage + ": " + e.detail), release: { available: r.available, not_confirmed: r.not_confirmed }, findings: f.findings.map((x) => ({ title: x.title, status: x.status, file: x.file })) };
}, id);
console.log(JSON.stringify(out, null, 1));
await browser.close(); server.kill(); process.exit(0);
