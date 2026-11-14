// Lists what a person can act on, screen by screen (helper for writing real-use runs; not part of any gate).
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { cpSync, mkdirSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const work = join(repo, "target/hunt-map");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), join(root, "payments-service"), { recursive: true });
renameSync(join(root, "payments-service/_env"), join(root, "payments-service/.env"));
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", "qwen2.5-coder:7b", "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d)); server.stderr.on("data", (d) => (log += d));
while (!/#pair=/.test(log)) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)[0];
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
await page.goto(link); await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" }); if (await skip.count()) await skip.click();
for (const r of process.argv.slice(2)) {
  await page.goto(new URL(link).origin + "/#/" + r); await page.waitForTimeout(1200);
  const items = await page.locator("main").locator("button, input, textarea, select, summary, [role=tab], a[href^='#/']").evaluateAll((els) => els.map((e) => `${e.tagName.toLowerCase()}${e.type ? ":" + e.type : ""} "${(e.getAttribute("aria-label") || e.labels?.[0]?.textContent || e.placeholder || e.textContent || "").replace(/\s+/g, " ").trim().slice(0, 60)}"${e.disabled ? " (off)" : ""}`));
  console.log(`\n== ${r}\n` + items.join("\n"));
}
await browser.close(); server.kill(); process.exit(0);
