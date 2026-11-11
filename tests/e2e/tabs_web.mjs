// Several conversations open as tabs: keep two conversations, switch between them by their tabs, close one (it stays kept),
// reload and find the tabs again.
//   node tests/e2e/tabs_web.mjs

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/tabs-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 31000 + (process.pid % 1000);
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

const post = (path, body) => page.evaluate(async ([path, body]) => {
  const csrf = (await (await fetch("/api/session", { credentials: "same-origin" })).json()).csrf;
  const r = await fetch(path, { method: "POST", headers: { "Content-Type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify(body) });
  return r.json();
}, [path, body]);
// two kept conversations, each with one exchange of its own
const a = (await post("/api/sessions", { title: "Pricing page" })).id;
const b = (await post("/api/sessions", { title: "Refund policy" })).id;
await post("/api/chat", { message: "alpha question about pricing", session: a });
await post("/api/chat", { message: "beta question about refunds", session: b });
// the two conversations were made behind the page's back, so the page is loaded again to list them
await page.goto(new URL(link).origin + "/#/chat");
await page.reload();
await page.waitForSelector("#chat-input");
await page.getByText("Pricing page").first().waitFor();
check((await page.getByRole("tablist", { name: "Open conversations" }).count()) === 0, "with nothing opened there is no tab strip");
const openIt = async (title) => { await page.locator("li", { hasText: title }).locator("button", { hasText: /^Open/ }).first().click(); };
await openIt("Pricing page");
await page.getByRole("tab", { name: "Pricing page" }).waitFor();
await openIt("Refund policy");
await page.getByRole("tab", { name: "Refund policy" }).waitFor();
const tabs = page.getByRole("tablist", { name: "Open conversations" });
check((await tabs.getByRole("tab").count()) === 3 && (await page.getByRole("tab", { name: "Refund policy" }).getAttribute("aria-selected")) === "true", "both conversations are tabs, the last opened one selected, plus a tab for a new chat");
await page.getByText("beta question about refunds").first().waitFor();
await page.getByRole("tab", { name: "Pricing page" }).click();
await page.getByText("alpha question about pricing").first().waitFor();
check((await page.locator("main").innerText()).includes("alpha question") && !(await page.locator("main").innerText()).includes("beta question about refunds"), "a click on a tab shows that conversation and not the other");
await page.fill("#chat-input", "half-written thought");
await page.getByRole("tab", { name: "Refund policy" }).click();
await page.getByText("beta question about refunds").first().waitFor();
await page.getByRole("tab", { name: "Pricing page" }).click();
await page.getByText("alpha question about pricing").first().waitFor();
check((await page.inputValue("#chat-input")) === "half-written thought", "what was being written in a conversation is still there after switching away and back");
await page.evaluate(() => window.scrollTo(0, 0));
await page.screenshot({ path: join(work, "1-tabs.png"), fullPage: false });
await page.reload();
await page.waitForSelector("#chat-input");
await page.getByRole("tab", { name: "Refund policy" }).waitFor();
check((await tabs.getByRole("tab").count()) === 3, "after a reload the tabs are still there");
await page.getByRole("button", { name: /Close the tab Refund policy/ }).click();
check((await page.getByRole("tab", { name: "Refund policy" }).count()) === 0, "closing a tab takes it off the strip");
check((await page.locator("li", { hasText: "Refund policy" }).locator("button", { hasText: /^Open/ }).count()) >= 1, "and the conversation is still kept in the list");
await page.setViewportSize({ width: 390, height: 800 });
check(!(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)), "the tab strip wraps on a phone-width screen");
check(errors.length === 0, "no console errors" + (errors.length ? ": " + errors.join(" | ") : ""));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
