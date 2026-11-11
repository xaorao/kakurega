// Sharing one finding as a person does it: a delivered review, "Save a share" on its Release tab, then the saved file dropped
// on the public check page, which is opened from disk and knows nothing of this service.
//   node tests/e2e/share_web.mjs      screenshots in target/share-web/

import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/share-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), join(root, "payments-service"), { recursive: true });
renameSync(join(root, "payments-service/_env"), join(root, "payments-service/.env"));
const built = spawnSync("node", ["build.mjs"], { cwd: join(repo, "apps/verifier-page"), encoding: "utf8" });
if (built.status !== 0) throw new Error(built.stderr);
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const data = join(work, "data");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", data, "--root", root, "--scripted", join(repo, "tests/e2e/script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const origin = new URL(link).origin;
const admin = JSON.parse(readFileSync(join(data, "admin.json"), "utf8"));
const adm = async (method, path, body) => {
  const r = await fetch(origin + path, { method, headers: { Authorization: "Bearer " + admin.token, "content-type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
  return r.json();
};

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1100 }, acceptDownloads: true });
const page = await ctx.newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
const post = (path, body) => page.evaluate(async ([path, body]) => {
  const csrf = (await (await fetch("/api/session", { credentials: "same-origin" })).json()).csrf;
  const r = await fetch(path, { method: "POST", headers: { "Content-Type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify(body) });
  return r.json();
}, [path, body]);
const id = (await post("/api/jobs", { repo: join(root, "payments-service"), recipient: "acme-audit" })).id;
for (let i = 0; i < 300; i++) {
  if ((await adm("GET", `/admin/jobs/${id}`)).state !== "running") break;
  await new Promise((r) => setTimeout(r, 300));
}
await page.goto(`${origin}/#/job/${id}`);
await page.getByRole("tab", { name: "Release" }).click().catch(async () => { await page.getByText("Release", { exact: true }).first().click(); });
check((await page.getByText("Share one finding with someone else").count()) === 0, "before delivery there is nothing to share");
const approval = (await post(`/api/jobs/${id}/approval`, {})).approval;
const shown = await adm("GET", `/admin/approvals/${approval}`);
await adm("POST", `/admin/approvals/${approval}/approve`, { ack: shown.digest.slice(0, 12) });
await post(`/api/jobs/${id}/deliver`, {});
await page.reload();
await page.getByText("Release", { exact: true }).first().click();
const section = page.locator(".share-one");
await section.waitFor({ timeout: 20000 });
const rows = section.locator("li[data-finding]");
check((await rows.count()) >= 1 && /Finding 1/.test(await rows.first().innerText()), "a delivered review lists its findings to share, each with its title");
await page.screenshot({ path: join(work, "1-release.png"), fullPage: true });
const [download] = await Promise.all([page.waitForEvent("download"), rows.first().getByRole("button", { name: "Save a share" }).click()]);
const saved = join(work, download.suggestedFilename());
await download.saveAs(saved);
check(/-finding-1\.kks$/.test(saved), "pressing Save a share saves a file named for the capsule and the finding");
await section.getByText(/^Saved /).waitFor();
const said = await section.innerText();
check(/It shows:/.test(said) && /It hides:/.test(said) && /Give the reader your signing key/.test(said), "the page says what the file shows, what it hides, and to pass the key on separately");
const key = (await section.locator("code").innerText()).trim();
await page.screenshot({ path: join(work, "2-saved.png"), fullPage: true });
check(problems.length === 0, "no console errors in the workspace: " + problems.join(" | "));

// ---- the reader: the public check page, from disk, with no knowledge of this service ----
const reader = await browser.newContext({ viewport: { width: 1000, height: 1000 } });
const rp = await reader.newPage();
const outside = [];
rp.on("request", (r) => { if (!r.url().startsWith("file:")) outside.push(r.url()); });
await rp.goto("file://" + join(repo, "apps/verifier-page/dist/kakurega-verify.html"));
await rp.setInputFiles("#file", saved);
await rp.getByText("Valid share").waitFor();
await rp.fill("#pin", key);
await rp.getByText("yes, matches the key you pasted").waitFor();
const body = await rp.locator("main").innerText();
check(/finding-1\.txt/.test(body) && /of the release's \d+ files/.test(body), "the reader's page finds the share valid, signed by the key the owner gave");
const shownFiles = await rp.locator("section.shared h3").allInnerTexts();
check(!/acme-audit/.test(body) && shownFiles.length > 0 && !shownFiles.some((h) => /^index\.json|^finding-2|^repro-2|^sample-2/.test(h)), "and shows nothing of the recipient or of the release's other files: " + shownFiles.join(", "));
check(outside.length === 0, "the check used no network: " + outside.join(", "));
await rp.screenshot({ path: join(work, "3-reader.png"), fullPage: true });
await browser.close();
server.kill();
