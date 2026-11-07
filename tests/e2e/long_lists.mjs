// Measures how the web screens behave with very long lists, before deciding whether a virtualiser is worth its cost.
// Not a pass/fail speed test of the machine: the budgets are generous and only catch a screen that has become unusable.
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/long-lists-tmp");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root/proj"), { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
let bad = 0;
const check = (n, ok, d = "") => { if (!ok) bad++; console.log(`${ok ? "PASS" : "FAIL"}  ${n}${d ? "  " + d : ""}`); };
const note = (s) => console.log("MEASURED  " + s);

const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", join(work, "root"), "--scripted", join(here, "script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let out = "";
server.stdout.on("data", (d) => (out += d));
server.stderr.on("data", (d) => (out += d));
const t = Date.now() + 30000;
while (!/#pair=/.test(out) && Date.now() < t) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(out)?.[0];
if (!link) { console.log("FAIL  kaku serve did not start:\n" + out); process.exit(1); }

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const problems = [];
try {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  await ctx.addInitScript(() => localStorage.setItem("kk-welcome", "done"));
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto(link);
  await page.getByRole("heading", { name: /Review a project|Finish setup first/ }).waitFor({ timeout: 20000 });

  // frames: the longest gap between animation frames while the page scrolls from top to bottom and back
  const scrollJank = (p) => p.evaluate(async () => {
    const gaps = [];
    let last = performance.now();
    const tick = () => { const n = performance.now(); gaps.push(n - last); last = n; };
    const el = document.scrollingElement;
    const total = el.scrollHeight;
    for (let y = 0; y <= total; y += 400) { el.scrollTop = y; await new Promise((r) => requestAnimationFrame(() => { tick(); r(); })); }
    for (let y = total; y >= 0; y -= 400) { el.scrollTop = y; await new Promise((r) => requestAnimationFrame(() => { tick(); r(); })); }
    gaps.sort((a, b) => a - b);
    return { frames: gaps.length, p95: gaps[Math.floor(gaps.length * 0.95)] ?? 0, max: gaps[gaps.length - 1] ?? 0 };
  });
  const heapMb = async (p) => { try { return Math.round(((await p.evaluate(() => performance.memory?.usedJSHeapSize)) ?? 0) / 1048576); } catch { return 0; } };

  // 1. the Memory screen with 5,000 notes
  const N = Number(process.env.KK_LONG_NOTES ?? 5000);
  const items = Array.from({ length: N }, (_, i) => ({ id: `mem-${i}`, text: `Synthetic note number ${i}: the customer prefers invoices in euros and pays on the ${1 + (i % 28)}th`, why: "you asked to remember it", untrusted: false, level: "internal", compartments: [], created_at: 1700000000 + i, valid_to: null, source: "user", project: i % 7 === 0 ? "kestrel" : null }));
  await page.route("**/api/memories?*", (r) => r.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ memories: items, total: N }) }));
  await page.route("**/api/memories", (r) => r.request().method() === "GET" ? r.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ memories: items, total: N }) }) : r.continue());
  const t0 = Date.now();
  await page.getByRole("link", { name: "Memory" }).click();
  await page.getByRole("heading", { name: "Memory", exact: true }).waitFor();
  await page.getByText(`Synthetic note number ${N - 1}:`, { exact: false }).first().waitFor({ timeout: 60000 });
  const memMs = Date.now() - t0;
  const memNodes = await page.evaluate(() => document.getElementsByTagName("*").length);
  const memJank = await scrollJank(page);
  note(`Memory, ${N} notes: shown after ${memMs} ms, ${memNodes} DOM nodes, ${await heapMb(page)} MB heap, scroll p95 ${memJank.p95.toFixed(0)} ms max ${memJank.max.toFixed(0)} ms over ${memJank.frames} frames`);
  check("Memory with a very long list is usable (shown within 15 s, no frame over 500 ms)", memMs < 15000 && memJank.max < 500, `${memMs} ms, max frame ${memJank.max.toFixed(0)} ms`);

  // 2. a long chat: each turn is sent through the page, the service's reply is the scripted one
  await page.unroute("**/api/memories?*");
  await page.unroute("**/api/memories");
  await page.getByRole("link", { name: "Chat" }).click();
  // the model's answer is replaced by an instant one, so what is measured is the page and not the model
  await page.route("**/api/chat/stream", (r) => r.fulfill({ status: 200, contentType: "application/x-ndjson", body: [
    JSON.stringify({ delta: "An answer with **some** words, a list:\n\n- one\n- two\n\nand `code`." }),
    JSON.stringify({ done: true, reply: "An answer with **some** words, a list:\n\n- one\n- two\n\nand `code`.", route: { id: "test", class: "device_only", simulated: true }, notes_used: 0, web_sources_used: 0, notes_withheld: 0, usage: { context_tokens_estimate: 100, reply_tokens_estimate: 20, history_turns_sent: 0, history_turns_dropped: 0 } }),
  ].join("\n") + "\n" }));
  const box = page.getByLabel("Your message");
  await box.waitFor();
  const lat = {};
  const TURNS = Number(process.env.KK_LONG_TURNS ?? 400);
  for (let k = 1; k <= TURNS; k++) {
    const before = await page.locator(".msg.assistant").count();
    const s = Date.now();
    await box.fill(`question number ${k} with some words in it`);
    await box.press("Enter");
    await page.locator(".msg.assistant").nth(before).getByRole("button", { name: "Copy", exact: true }).waitFor({ timeout: 30000 });
    if ([1, 100, 200, TURNS].includes(k)) lat[k] = Date.now() - s;
  }
  const chatNodes = await page.evaluate(() => document.getElementsByTagName("*").length);
  const chatJank = await scrollJank(page);
  note(`Chat, ${TURNS} turns: time to answer at turn 1/100/200/${TURNS} = ${[1, 100, 200, TURNS].map((k) => lat[k]).join("/")} ms, ${chatNodes} DOM nodes, ${await heapMb(page)} MB heap, scroll p95 ${chatJank.p95.toFixed(0)} ms max ${chatJank.max.toFixed(0)} ms`);
  check("a long chat still answers within 5 s and scrolls without a frame over 500 ms", lat[TURNS] < 5000 && chatJank.max < 500, `turn ${TURNS} ${lat[TURNS]} ms, max frame ${chatJank.max.toFixed(0)} ms`);
  check("no page errors", problems.length === 0, problems.join(" | "));
  await ctx.close();
} finally {
  await browser.close();
  server.kill();
}
console.log(bad === 0 ? "\nlong-list checks passed" : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
