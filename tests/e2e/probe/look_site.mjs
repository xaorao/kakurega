// Screenshots of the built public site for reading with the eyes (helper; not part of any gate).
//   node tests/e2e/probe/look_site.mjs [light|dark] [WIDTH] [PATH]     writes target/look/site-*.png (the fold and the full page)
import { chromium } from "playwright-core";
import { createServer } from "node:http";
import { readFileSync, existsSync, statSync, mkdirSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const dist = join(repo, "apps/site/dist");
const [theme = "light", width = "1440", path = "/"] = process.argv.slice(2);
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm", ".webp": "image/webp", ".png": "image/png", ".woff2": "font/woff2" };
const server = createServer((req, res) => {
  let p = join(dist, decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (existsSync(p) && statSync(p).isDirectory()) p = join(p, "index.html");
  if (!existsSync(p)) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { "content-type": types[extname(p)] ?? "application/octet-stream" });
  res.end(readFileSync(p));
}).listen(0);
const origin = `http://127.0.0.1:${server.address().port}`;
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: Number(width), height: 900 }, colorScheme: theme === "dark" ? "dark" : "light" })).newPage();
page.on("console", (m) => { if (m.type() === "error") console.log("console: " + m.text()); });
page.on("pageerror", (e) => console.log("pageerror: " + e.message));
await page.goto(origin + path);
await page.waitForTimeout(3200);
mkdirSync(join(repo, "target/look"), { recursive: true });
const name = `site-${theme}-${width}${path.replace(/\//g, "_")}`;
await page.screenshot({ path: join(repo, `target/look/${name}fold.png`) });
// every part is revealed before the long picture is taken
await page.evaluate(() => document.querySelectorAll(".band, .quote, .proof-steps > li, .try-steps > li, .try-notes").forEach((b) => b.classList.add("in")));
await page.waitForTimeout(900);
await page.screenshot({ path: join(repo, `target/look/${name}full.png`), fullPage: true });
console.log(name, await page.evaluate(() => document.documentElement.scrollWidth > innerWidth ? "SCROLLS SIDEWAYS" : "ok"));
await browser.close(); server.close(); process.exit(0);
