// Reviewing a change as a person does it on the web: New review, "Only a change", the preview, the review, approval and
// delivery, then "Copy as a pull-request comment", and the capsule checked the way the comment tells its reader to.
//   node tests/e2e/change_web.mjs      screenshots in target/change-web/

import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/change-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const proj = join(root, "payments-service");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), proj, { recursive: true });
rmSync(join(proj, "_env"));
const git = (...args) => {
  const r = spawnSync("/usr/bin/git", ["-C", proj, ...args], { encoding: "utf8", env: { ...process.env, GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@example.com", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@example.com", GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_SYSTEM: "/dev/null" } });
  if (r.status !== 0) throw new Error(r.stderr);
  return r.stdout.trim();
};
const session = join(proj, "payments/auth/session.py");
const broken = readFileSync(session, "utf8");
const fixed = broken.replace('    # Clears the browser cookie on the client side; the server keeps the entry.\n    cookie_jar = {"session": token}\n    cookie_jar.pop("session", None)\n', "    ACTIVE_TOKENS.pop(token, None)\n");
if (fixed === broken) throw new Error("the fixture changed");
writeFileSync(session, fixed);
git("init", "-q", "-b", "main");
git("add", "-A");
git("commit", "-q", "-m", "payments service");
const baseCommit = git("rev-parse", "HEAD");
git("checkout", "-q", "-b", "faster-logout");
writeFileSync(session, broken);
git("commit", "-q", "-am", "logout: skip the table");
const headCommit = git("rev-parse", "HEAD");
// an edit that was never committed: not part of the change, and the page must say so
writeFileSync(join(proj, "README.md"), readFileSync(join(proj, "README.md"), "utf8") + "\nedited after the commit\n");

const built = spawnSync("node", ["build.mjs"], { cwd: join(repo, "apps/verifier-page"), encoding: "utf8" });
if (built.status !== 0) throw new Error(built.stderr);
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const data = join(work, "data");
const kaku = join(repo, "target/debug/kaku");
const server = spawn(kaku, ["serve", "--data-dir", data, "--root", root, "--scripted", join(repo, "tests/e2e/change-script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
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
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1100 }, acceptDownloads: true, permissions: ["clipboard-read", "clipboard-write"] });
const page = await ctx.newPage();
const problems = [];
// the one refusal this test asks for (a base that does not exist) is a 400 the browser reports by itself
page.on("console", (m) => { if (m.type() === "error" && !/status of 400/.test(m.text())) problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
// a reload ends the page's open requests; anything else that fails is a problem
page.on("requestfailed", (r) => { if (r.failure()?.errorText !== "net::ERR_ABORTED") problems.push("requestfailed: " + r.url() + " " + r.failure()?.errorText); });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.goto(origin + "/#/");
await page.getByRole("button", { name: "payments-service", exact: true }).click();
await page.locator(".chosen, [data-chosen]").first().waitFor({ timeout: 5000 }).catch(() => {});

// a wrong base is said in words, on the page
await page.getByLabel("Only a change (a branch against its base, as for a pull request)").check();
await page.getByLabel("Measured from (a branch, a tag or a commit)").fill("no-such-branch");
await page.getByRole("button", { name: "Show the change" }).click();
await page.getByRole("alert").filter({ hasText: "no branch, tag or commit of that name" }).waitFor();
check(true, "a base that does not exist is refused in words on the page");
await page.getByLabel("Measured from (a branch, a tag or a commit)").fill("main");
check((await page.getByRole("alert").count()) === 0, "typing a new base clears the old refusal");
await page.getByRole("button", { name: "Show the change" }).click();
await page.getByText("1 file changed").waitFor();
const preview = await page.locator(".change-list").innerText();
check(/payments\/auth\/session\.py/.test(preview) && /modified · lines 16-18/.test(preview), "the preview lists the changed file and the lines the change wrote: " + preview);
check((await page.getByText(`Commit ${headCommit.slice(0, 12)} measured from commit ${baseCommit.slice(0, 12)}`).count()) === 1, "and the two commits, as git names them");
check((await page.getByText("Files edited since the last commit: 1. Those edits are not reviewed").count()) === 1, "the preview says one committed file was edited since and is not reviewed");
await page.screenshot({ path: join(work, "1-new-review.png"), fullPage: true });
await page.getByLabel("Recipient").fill("acme-audit").catch(async () => { await page.locator("#recipient").fill("acme-audit"); });
await page.getByRole("button", { name: /^Start/ }).click();
await page.waitForURL(/#\/job\/job-\d+/);
const id = /job-\d+/.exec(page.url())[0];
for (let i = 0; i < 300; i++) {
  if ((await adm("GET", `/admin/jobs/${id}`)).state !== "running") break;
  await new Promise((r) => setTimeout(r, 300));
}
const post = (path, body) => page.evaluate(async ([path, body]) => {
  const csrf = (await (await fetch("/api/session", { credentials: "same-origin" })).json()).csrf;
  const r = await fetch(path, { method: "POST", headers: { "Content-Type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify(body) });
  return r.json();
}, [path, body]);
await page.reload();
await page.getByText("Release", { exact: true }).first().click();
await page.getByText("change.txt").first().waitFor();
check((await page.getByText("Copy as a pull-request comment").count()) === 0, "before delivery there is no comment to copy");
await page.screenshot({ path: join(work, "2-release.png"), fullPage: true });
const approval = (await post(`/api/jobs/${id}/approval`, {})).approval;
const shown = await adm("GET", `/admin/approvals/${approval}`);
await adm("POST", `/admin/approvals/${approval}/approve`, { ack: shown.digest.slice(0, 12) });
await post(`/api/jobs/${id}/deliver`, {});
await page.reload();
await page.getByText("Release", { exact: true }).first().click();
const copy = page.getByRole("button", { name: "Copy as a pull-request comment" });
await copy.waitFor({ timeout: 20000 });
await copy.click();
await page.getByRole("button", { name: "Copied" }).waitFor();
const clip = await page.evaluate(() => navigator.clipboard.readText());
check(clip.startsWith("### Private security review: 1 confirmed finding\n"), "the copied comment counts the one finding that touches the change");
check(clip.includes(`Reviewed commit \`${headCommit}\`, measured from \`${baseCommit}\``), "names the two commits");
check(clip.includes("Token still valid after logout") && clip.includes("`payments/auth/session.py:15`") && !/forged|tokens\.py/.test(clip), "holds the finding in the change and nothing of the one outside it");
check(!clip.includes("session.login(") && !clip.includes("ACTIVE_TOKENS"), "and none of the project's code");
await page.getByRole("button", { name: "Show the comment" }).click();
const shownText = await page.locator("pre.pr-comment").innerText();
check(shownText.trim() === clip.trim(), "the comment shown on the page is the one that was copied");
await page.screenshot({ path: join(work, "3-delivered.png"), fullPage: true });

// do what the comment tells its reader: check the capsule offline, and re-run the script inside it
const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("link", { name: "Download the capsule" }).click()]);
const capsule = join(work, "review.kkg");
await download.saveAs(capsule);
const named = /capsule `([0-9a-f]{32})`/.exec(clip)?.[1] ?? "";
const v = spawnSync(kaku, ["verify", capsule], { encoding: "utf8" });
check(v.status === 0, "`kaku verify review.kkg` accepts the capsule: " + (v.stdout + v.stderr).slice(-300));
check(named.length === 32 && v.stdout.includes(`capsule ${named}\n`), "and prints the capsule name the comment gives: " + named);
check(v.stdout.includes("1. Token still valid after logout") && v.stdout.includes(`2. This review is of a change: commit ${headCommit.slice(0, 12)} measured from commit ${baseCommit.slice(0, 12)}.`), "and each signed claim: the finding and the change");
// the same capsule on the public check page, opened from disk
const reader = await browser.newContext({ viewport: { width: 1000, height: 1200 } });
const rp = await reader.newPage();
await rp.goto("file://" + join(repo, "apps/verifier-page/dist/kakurega-verify.html"));
await rp.setInputFiles("#file", capsule);
await rp.getByText("What the signer claims").waitFor();
const claimed = await rp.locator("main").innerText();
check(claimed.includes("Token still valid after logout") && claimed.includes("This review is of a change: commit") && claimed.includes("Rests on: change.txt"), "the check page lists the same claims, with what each rests on");
await rp.screenshot({ path: join(work, "4-check-page.png"), fullPage: true });
check(problems.length === 0, "no console errors: " + problems.join(" | "));
await browser.close();
server.kill();
