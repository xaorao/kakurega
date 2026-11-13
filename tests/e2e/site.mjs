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
const types = { ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript", ".json": "application/json", ".png": "image/png", ".kkg": "application/octet-stream" };
// the headers the host will send (vercel.json), sent here too, so the policy is proven not to break the pages
const rules = JSON.parse(readFileSync(join(dir, "vercel.json"), "utf8")).headers.map((h) => ({ re: new RegExp("^" + h.source + "$"), headers: h.headers }));
const asked = [];
const srv = createServer((q, r) => {
  asked.push(q.url);
  let p = join(dir, decodeURIComponent(q.url.split("?")[0]));
  if (!p.startsWith(dir)) { r.statusCode = 400; return r.end(); }
  if (existsSync(p) && statSync(p).isDirectory()) p = join(p, "index.html");
  if (!existsSync(p)) { r.statusCode = 404; return r.end("not found"); }
  r.setHeader("content-type", types[extname(p)] ?? "application/octet-stream");
  const path = q.url.split("?")[0];
  for (const rule of rules) if (rule.re.test(path)) for (const h of rule.headers) r.setHeader(h.key, h.value);
  r.end(readFileSync(p));
}).listen(0, "127.0.0.1");
await new Promise((r) => srv.on("listening", r));
// SITE_URL checks the deployed site in place of the local copy: the same visitor, the real host and its real headers
const origin = (process.env.SITE_URL ?? `http://127.0.0.1:${srv.address().port}`).replace(/\/$/, "");
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
    // the replay: every fact in it comes from the capsule, and the last step checks the capsule here
    const facts = JSON.parse(readFileSync(join(dir, "replay.json"), "utf8"));
    const told = spawnSync(join(repo, "target/debug/kaku"), ["verify", join(dir, "sample/review.kkg")], { encoding: "utf8" });
    check(told.status === 0 && told.stdout.includes(`signer ${facts.model_key.slice(0, 12)}`) && told.stdout.includes(`Model answers signed by their model (${facts.answers})`), "the page's facts about the capsule are what `kaku verify` says about it: " + facts.model_key.slice(0, 12) + ", " + facts.answers + " answers");
    const hero = await page.locator(".hero").innerText();
    check((await page.locator(".receipt").innerText()).includes(facts.model_key) && (await page.locator("#check .lede").innerText()).includes(`${facts.answers} answers`), "the receipt card names that key and that number of answers");
    const tab = (n) => page.getByRole("tab", { name: new RegExp("^" + n + "\\.") });
    await tab(1).focus();
    await page.keyboard.press("ArrowRight");
    check((await tab(2).getAttribute("aria-selected")) === "true" && (await page.locator("#rp-2").isVisible()) && !(await page.locator("#rp-1").isVisible()), "the steps of the replay can be walked with the arrow keys");
    const shownFindings = await page.locator("#rp-2 .findings li strong").allInnerTexts();
    check(shownFindings.length === facts.findings.length && facts.findings.every((f, i) => shownFindings[i].startsWith(f.title)), `step 2 lists the capsule's ${facts.findings.length} findings by their titles`);
    await tab(3).click();
    check((await page.locator("#rp-3 tbody tr").count()) === facts.files.length && facts.files.length >= 5, `step 3 lists the release's ${facts.files.length} files`);
    await tab(4).click();
    check(await page.getByRole("button", { name: "Change one byte and check again" }).isDisabled(), "there is nothing to break before the real check has run");
    await page.getByRole("button", { name: "Check the real capsule" }).click();
    await page.getByText("Valid: every signature checks out").waitFor({ timeout: 20000 });
    check((await page.locator("[data-verdict]").innerText()).includes(`${facts.answers} answers signed by the model's own key`), "step 4 checks the real capsule in the page and counts its signed answers");
    await page.getByRole("button", { name: "Change one byte and check again" }).click();
    await page.getByText("Refused: the file is not what was signed").waitFor({ timeout: 20000 });
    check(true, "and refuses it with one byte changed");
    await page.screenshot({ path: join(work, "replay-refused.png") });
    check(/Live on NEAR testnet/.test(hero) && /built on\s+NEAR AI Cloud/i.test(hero.replace(/\s+/g, " ")), "the top says where it is live and what it is built on");

    // do what the page says: download the capsule, drop it on the check page
    const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("link", { name: "1. Download the capsule" }).click()]);
    const capsule = join(work, "review.kkg");
    await download.saveAs(capsule);
    await page.getByRole("link", { name: /^2\. Drop it on the check page/ }).click();
    await page.waitForURL(/\/verify\/$/);
    await page.setInputFiles("#file", capsule);
    await page.getByText("Valid: the signatures check out").waitFor({ timeout: 20000 });
    const body = await page.locator("main").innerText();
    check(body.includes(facts.model) && body.includes(facts.model_key.slice(0, 12)), "the sample capsule is valid on the check page and shows the model and the key the landing page names");
    check(body.split(facts.model).length - 1 >= facts.answers, `it lists the ${facts.answers} signed answers the landing page promises`);
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
if (process.env.SITE_URL) {
  for (const hidden of ["/vercel.json", "/.env.local", "/.gitignore", "/.vercel/project.json"]) {
    const r = await fetch(origin + hidden);
    check(r.status === 404, `the host does not serve ${hidden}: ${r.status}`);
  }
  const head = await fetch(origin + "/");
  check(/default-src 'none'/.test(head.headers.get("content-security-policy") ?? "") && head.headers.get("x-frame-options") === "DENY", "the host sends the page's security policy");
}
await browser.close();
srv.close();
