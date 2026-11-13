// The public site as a visitor meets it, served as plain files: every link leads somewhere, the sample capsule checks
// out on the check page and a changed one is refused, nothing is asked of any other host, and it reads at three sizes.
//   node tests/e2e/site.mjs      screenshots in target/site/

import { chromium } from "playwright-core";
import { createServer } from "node:http";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/site");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
const built = spawnSync("node", [join(repo, "apps/site/build.mjs")], { encoding: "utf8" });
if (built.status !== 0) throw new Error(built.stderr);
const dir = join(repo, "apps/site/dist");
const types = { ".html": "text/html; charset=utf-8", ".css": "text/css", ".png": "image/png", ".kkg": "application/octet-stream" };
const asked = [];
const srv = createServer((q, r) => {
  asked.push(q.url);
  let p = join(dir, decodeURIComponent(q.url.split("?")[0]));
  if (!p.startsWith(dir)) { r.statusCode = 400; return r.end(); }
  if (existsSync(p) && statSync(p).isDirectory()) p = join(p, "index.html");
  if (!existsSync(p)) { r.statusCode = 404; return r.end("not found"); }
  r.setHeader("content-type", types[extname(p)] ?? "application/octet-stream");
  r.end(readFileSync(p));
}).listen(0, "127.0.0.1");
await new Promise((r) => srv.on("listening", r));
const origin = `http://127.0.0.1:${srv.address().port}`;
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

for (const [name, viewport, scheme] of [["desktop-light", { width: 1440, height: 900 }, "light"], ["desktop-dark", { width: 1440, height: 900 }, "dark"], ["phone-light", { width: 390, height: 844 }, "light"]]) {
  const ctx = await browser.newContext({ viewport, colorScheme: scheme, acceptDownloads: true });
  const page = await ctx.newPage();
  const problems = [];
  const outside = [];
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text().slice(0, 160)); });
  page.on("pageerror", (e) => problems.push(e.message.slice(0, 160)));
  page.on("request", (r) => { if (!r.url().startsWith(origin) && !r.url().startsWith("data:") && !r.url().startsWith("blob:")) outside.push(r.url()); });
  page.on("response", (r) => { if (r.status() >= 400) problems.push(r.status() + " " + r.url()); });
  await page.goto(origin + "/");
  await page.waitForTimeout(1900);
  check(/Private AI work\s+you can prove\./.test((await page.locator("h1").innerText()).replace(/\s+/g, " ")), `${name}: the headline reads whole once the bar has lifted`);
  const images = await page.evaluate(() => [...document.images].map((i) => ({ src: i.getAttribute("src"), ok: i.complete && i.naturalWidth > 0, alt: i.alt.length })));
  check(images.length >= 3 && images.every((i) => i.ok && i.alt > 20), `${name}: every picture loads and is described: ${JSON.stringify(images.filter((i) => !i.ok || i.alt <= 20))}`);
  const wide = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1);
  check(!wide, `${name}: the page does not scroll sideways`);
  // every link: an anchor on the page, or a file that is there
  const links = await page.evaluate(() => [...document.querySelectorAll("a[href]")].map((a) => a.getAttribute("href")));
  const broken = [];
  for (const h of new Set(links)) {
    if (h.startsWith("#")) { if (!(await page.locator(h).count())) broken.push(h); continue; }
    if (/^https?:/.test(h)) { broken.push("outside link " + h); continue; }
    const r = await fetch(origin + "/" + h.replace(/^\.\//, ""));
    if (r.status !== 200) broken.push(h + " " + r.status);
  }
  check(broken.length === 0, `${name}: every link leads somewhere (${new Set(links).size} links): ${broken.join(", ")}`);
  await page.screenshot({ path: join(work, `${name}-top.png`) });
  await page.screenshot({ path: join(work, `${name}-full.png`), fullPage: true });

  if (name === "desktop-light") {
    // do what the page says: download the capsule, drop it on the check page
    const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("link", { name: "1. Download the capsule" }).click()]);
    const capsule = join(work, "review.kkg");
    await download.saveAs(capsule);
    await page.getByRole("link", { name: /^2\. Drop it on the check page/ }).click();
    await page.waitForURL(/\/verify\/$/);
    await page.setInputFiles("#file", capsule);
    await page.getByText("Valid: the signatures check out").waitFor({ timeout: 20000 });
    const body = await page.locator("main").innerText();
    check(/Qwen\/Qwen3\.8-27B/.test(body) && /89154a43a476/.test(body), "the sample capsule is valid on the check page and shows the model and the key the landing page names");
    check((body.match(/Qwen\/Qwen3\.8-27B/g) ?? []).length >= 20, "it lists the twenty signed answers the landing page promises");
    const bytes = readFileSync(capsule);
    bytes[Math.floor(bytes.length / 2)] ^= 0x01;
    const bad = join(work, "changed.kkg");
    writeFileSync(bad, bytes);
    await page.setInputFiles("#file", bad);
    await page.getByText("Not valid").waitFor({ timeout: 20000 });
    check(true, "one changed byte and it is refused, as the page says");
    await page.screenshot({ path: join(work, "verify-refused.png") });
  }
  check(outside.length === 0, `${name}: nothing was asked of any other host: ${outside.slice(0, 3).join(", ")}`);
  check(problems.length === 0, `${name}: no console errors or failed requests: ${problems.join(" | ")}`);
  await ctx.close();
}
await browser.close();
srv.close();
