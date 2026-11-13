// Screenshots of chosen screens for a person to read: desktop and phone, light and dark, with the scripted test model.
//   node tests/e2e/shots.mjs '#/' '#/chat' ...      pictures in target/shots/
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/shots");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), join(work, "root/payments-service"), { recursive: true });
renameSync(join(work, "root/payments-service/_env"), join(work, "root/payments-service/.env"));
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", join(work, "root"), "--scripted", join(repo, "tests/e2e/script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const origin = new URL(/http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)[0]).origin;
const admin = JSON.parse(readFileSync(join(work, "data/admin.json"), "utf8"));
const hashes = process.argv.slice(2).length ? process.argv.slice(2) : ["#/", "#/chat"];
const browser = await chromium.launch({ executablePath: chrome, headless: true });
for (const [name, viewport, scheme] of [["desktop-light", { width: 1280, height: 860 }, "light"], ["desktop-dark", { width: 1280, height: 860 }, "dark"], ["phone-light", { width: 390, height: 844 }, "light"]]) {
  const ctx = await browser.newContext({ viewport, colorScheme: scheme });
  const page = await ctx.newPage();
  const problems = [];
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text().slice(0, 160)); });
  page.on("pageerror", (e) => problems.push(e.message.slice(0, 160)));
  const code = (await (await fetch(origin + "/admin/pairing", { method: "POST", headers: { Authorization: "Bearer " + admin.token, "content-type": "application/json" }, body: "{}" })).json()).code;
  await page.goto(`${origin}/#pair=${code}`);
  await page.getByRole("button", { name: "Skip the rest" }).click({ timeout: 6000 }).catch(() => {});
  for (const h of hashes) {
    await page.goto(origin + "/" + h);
    await page.waitForTimeout(900);
    const file = `${name}${h.replace(/[^a-z0-9]+/gi, "-")}.png`;
    await page.screenshot({ path: join(work, file) });
    const wide = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1);
    console.log(file, wide ? "SCROLLS SIDEWAYS" : "", problems.splice(0).join(" | "));
  }
  await ctx.close();
}
await browser.close();
server.kill();
