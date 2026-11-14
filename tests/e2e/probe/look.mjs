// Screenshots of the workspace for reading with the eyes (helper; not part of any gate). It serves a copy of the data a
// filmed review left in target/film-flow, so the review screens show a real finished review.
//   node tests/e2e/probe/look.mjs [light|dark] [WIDTH] ROUTE[@Tab]...     e.g.  look.mjs light 1440 job@Progress job@Release chat
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { cpSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [theme = "light", width = "1440", ...shots] = process.argv.slice(2);
const work = join(repo, "target/look-data");
const out = join(repo, "target/look");
rmSync(work, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
cpSync(join(repo, "target/film-flow/data"), join(work, "data"), { recursive: true });
cpSync(join(repo, "target/film-flow/root"), join(work, "root"), { recursive: true });
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", join(work, "root"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d)); server.stderr.on("data", (d) => (log += d));
while (!/#pair=/.test(log)) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)[0];
const origin = new URL(link).origin;
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: Number(width), height: 900 }, colorScheme: theme === "dark" ? "dark" : "light", reducedMotion: "reduce" })).newPage();
page.on("pageerror", (e) => console.log("pageerror: " + e.message));
// the two screens seen before the workspace: the pairing page (no link) and the welcome
if (shots.includes("pair")) { const fresh = await (await browser.newContext({ viewport: { width: Number(width), height: 900 }, colorScheme: theme === "dark" ? "dark" : "light" })).newPage(); await fresh.goto(origin); await fresh.waitForTimeout(1500); await fresh.screenshot({ path: join(out, `${theme}-${width}-pair.png`) }); console.log("pair"); }
await page.goto(link);
if (shots.includes("welcome")) { await page.waitForTimeout(2500); await page.screenshot({ path: join(out, `${theme}-${width}-welcome.png`) }); console.log("welcome"); }
await page.getByRole("button", { name: "Skip the rest" }).click({ timeout: 6000 }).catch(() => {});
if (shots.includes("palette")) { await page.waitForTimeout(1200); await page.getByRole("button", { name: "Commands" }).click(); await page.waitForTimeout(600); await page.keyboard.type("re"); await page.waitForTimeout(400); await page.screenshot({ path: join(out, `${theme}-${width}-palette.png`) }); await page.keyboard.press("Escape"); console.log("palette"); }
for (const shot of shots.filter((x) => !["pair", "welcome", "palette"].includes(x))) {
  const [route, tab] = shot.split("@");
  await page.goto(origin + (route === "job" ? "/#/job/job-1" : route === "start" ? "/#/" : "/#/" + route));
  await page.waitForTimeout(1500);
  if (tab) { await page.getByRole("tab", { name: tab }).click(); await page.waitForTimeout(1200); }
  if (process.env.AT) { const [x, y] = process.env.AT.split(",").map(Number); console.log(await page.evaluate(([x, y]) => document.elementsFromPoint(x, y).slice(0, 5).map((e) => `${e.tagName}.${e.className} ${getComputedStyle(e).borderLeft} / ${getComputedStyle(e).borderRight}`).join("\n"), [x, y])); }
  const name = `${theme}-${width}-${route}${tab ? "-" + tab : ""}.png`;
  await page.screenshot({ path: join(out, name) });
  console.log(name);
}
await browser.close(); server.kill(); process.exit(0);
