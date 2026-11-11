// The workspace's own check page takes a one-finding share as well as a capsule: a real share is made from a delivered review
// (as tests/e2e/share_web.mjs does), loaded on /verify, found valid with its finding shown as text; then a copy with one
// character of a shared file changed inside the JSON, and a file with markup put in its place, are both refused in words.
//   node tests/e2e/verify_inapp.mjs      screenshots in target/verify-inapp/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { generateKeyPairSync } from "node:crypto";
import { cpSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/verify-inapp");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), join(root, "payments-service"), { recursive: true });
renameSync(join(root, "payments-service/_env"), join(root, "payments-service/.env"));
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
page.on("dialog", (d) => { problems.push("dialog: " + d.message()); void d.dismiss(); });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

// ---- a real share, made the way the owner makes one ----
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
const approval = (await post(`/api/jobs/${id}/approval`, {})).approval;
const shownApproval = await adm("GET", `/admin/approvals/${approval}`);
await adm("POST", `/admin/approvals/${approval}/approve`, { ack: shownApproval.digest.slice(0, 12) });
await post(`/api/jobs/${id}/deliver`, {});
await page.goto(`${origin}/#/job/${id}`);
await page.getByText("Release", { exact: true }).first().click();
const section = page.locator(".share-one");
await section.waitFor({ timeout: 20000 });
const rows = section.locator("li[data-finding]");
const [download] = await Promise.all([page.waitForEvent("download"), rows.first().getByRole("button", { name: "Save a share" }).click()]);
const saved = join(work, download.suggestedFilename());
await download.saveAs(saved);
await section.getByText(/^Saved /).waitFor();
const key = (await section.locator("code").innerText()).trim();
check(/\.kks$/.test(saved), "a real share was saved from a delivered review: " + saved.split("/").pop());

const share = JSON.parse(readFileSync(saved, "utf8"));
const b64 = (s) => Buffer.from(s.replace(/-/g, "+").replace(/_/g, "/"), "base64");
const finding = share.leaves.find((l) => /^finding-/.test(l.name)) ?? share.leaves[0];
const findingText = b64(finding.content).toString("utf8");

// ---- the workspace's own check page ----
await page.goto(origin + "/verify");
await page.locator("#file").waitFor();
await page.setInputFiles("#file", saved);
const result = page.locator("#share-result");
await result.getByRole("heading", { name: "Valid", exact: true }).waitFor({ timeout: 10000 });
let body = await result.innerText();
check(true, "the share is found valid on the in-app check page");
check(new RegExp(`Each of the ${share.leaves.length} shared file\\(s\\) is exactly one of the \\d+ files`).test(body), "it says how many of the release's files were shared");
check(/capsule it comes from has the SHA-256 digest [0-9a-f]{64}/.test(body), "it names the capsule's digest");
check(/no key was pinned/.test(body), "before a key is pinned it says the key shown comes from the share itself");
const names = await result.locator("section.shared-file .shared-name code").allInnerTexts();
check(names.length === share.leaves.length && share.leaves.every((l) => names.includes(l.name)), "every shared file is listed by its exact name: " + names.join(", "));
const pre = result.locator(`section.shared-file[aria-label="${finding.name}"] pre`);
const preText = await pre.innerText();
const firstLine = findingText.split("\n").find((l) => l.trim()) ?? "";
check(firstLine.length > 0 && preText.includes(firstLine.trim()), "the shared finding is shown: " + firstLine.slice(0, 60));
check((await pre.evaluate((e) => e.children.length)) === 0, "and it is shown as text only, with no markup inside it");
check(!/acme-audit/.test(await page.locator("main").innerText()), "nothing of the recipient is shown");
await page.fill("#pin", key);
await page.locator("#pin").press("Tab");
await result.getByText("The signing key is the one you pinned.").waitFor({ timeout: 8000 });
body = await result.innerText();
check(!/no key was pinned/.test(body), "with the owner's key pinned it says the key is the one pinned");
await page.screenshot({ path: join(work, "1-valid-share.png"), fullPage: true });
await page.setViewportSize({ width: 390, height: 900 });
await page.screenshot({ path: join(work, "1b-valid-share-phone.png"), fullPage: true });
const wide = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
check(!wide, "on a phone-width screen the page does not scroll sideways");
await page.setViewportSize({ width: 1280, height: 1100 });

// a pasted key that is not the signer's: refused, in words
const other = generateKeyPairSync("ed25519").publicKey.export({ format: "jwk" }).x;
await page.fill("#pin", other);
await page.locator("#pin").press("Tab");
await page.getByText("Not valid", { exact: true }).waitFor({ timeout: 8000 });
check(/untrusted_signer/.test(await page.locator("main").innerText()), "a pinned key that did not sign it is refused");
await page.fill("#pin", "");
await page.locator("#pin").press("Tab");

// ---- a copy with one character of the shared file changed inside the JSON ----
const raw = readFileSync(saved, "utf8");
const at = raw.indexOf(finding.content) + Math.floor(finding.content.length / 2);
const was = raw[at];
const now = was === "A" ? "B" : "A";
const tampered = join(work, "tampered.kks");
writeFileSync(tampered, raw.slice(0, at) + now + raw.slice(at + 1));
await page.setInputFiles("#file", tampered);
await result.getByRole("heading", { name: "Not valid", exact: true }).waitFor({ timeout: 10000 });
body = await result.innerText();
check(/not part of the signed release/.test(body) && /not_in_release/.test(body), "a share with one changed character is refused in words: " + body.split("\n").slice(0, 3).join(" / "));
check((await result.locator("pre").count()) === 0, "and none of its file is shown");
await page.screenshot({ path: join(work, "2-refused-share.png"), fullPage: true });

// ---- a copy whose shared file is replaced with markup: refused, and nothing of it is drawn as markup ----
const markup = Buffer.from('<img src=x onerror="alert(1)"><b id="injected">bold</b>').toString("base64url");
const swapped = join(work, "markup.kks");
writeFileSync(swapped, raw.replace(finding.content, markup));
await page.setInputFiles("#file", swapped);
await result.getByRole("heading", { name: "Not valid", exact: true }).waitFor({ timeout: 10000 });
check((await page.locator("#injected").count()) === 0 && (await page.locator("main img").count()) === 0, "a share with markup put in its file is refused and nothing of it is drawn");

check(problems.length === 0, "no console errors, page errors or dialogs: " + problems.join(" | "));
await browser.close();
server.kill();
process.exit(process.exitCode ?? 0);
