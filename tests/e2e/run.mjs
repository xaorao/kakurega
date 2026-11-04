import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/e2e-tmp");
const shots = join(repo, "target/e2e-shots");
const kaku = join(repo, "target/debug/kaku");
rmSync(work, { recursive: true, force: true });
mkdirSync(shots, { recursive: true });
const root = join(work, "root");
const project = join(root, "payments-service");
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
const data = join(work, "data");

const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  " + detail : ""}`);
};

const server = spawn(kaku, ["serve", "--data-dir", data, "--root", root, "--scripted", join(here, "script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let out = "";
server.stdout.on("data", (d) => (out += d));
server.stderr.on("data", (d) => (out += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(out) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(out)?.[0];
assert.ok(link, "kaku serve did not print a pairing link:\n" + out);
const base = new URL(link).origin;

const problems = [];
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } });
const page = await ctx.newPage();
page.on("console", (m) => m.type() === "error" && problems.push("console: " + m.text()));
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => problems.push("requestfailed: " + r.url() + " " + r.failure()?.errorText));
page.on("response", (r) => r.status() >= 500 && problems.push("http " + r.status() + " " + r.url()));

try {
  await page.goto(link);
  await page.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 15000 });
  check("pairing link signs the browser in and clears the code from the address bar", !page.url().includes("pair="));
  await page.screenshot({ path: join(shots, "01-new-review.png") });

  const other = await browser.newContext();
  const reuse = await other.newPage();
  await reuse.goto(link);
  await reuse.getByRole("heading", { name: "Pair this browser" }).waitFor({ timeout: 15000 });
  await reuse.getByRole("alert").waitFor({ timeout: 15000 });
  check("a used pairing link does not work a second time", /wrong, used or expired/.test(await reuse.getByRole("alert").innerText()));
  await other.close();

  await page.getByRole("button", { name: "payments-service" }).click();
  await page.waitForFunction(() => document.querySelector(".path")?.textContent?.endsWith("payments-service"));
  await page.getByLabel("Recipient name").fill("acme-audit");
  await page.getByRole("button", { name: "Start review" }).click();
  await page.getByRole("tab", { name: "Release" }).waitFor({ timeout: 120000 });
  await page.getByRole("heading", { name: "The exact release" }).waitFor({ timeout: 120000 });
  check("review runs in the sandbox and opens on the release", true);
  await page.screenshot({ path: join(shots, "02-release.png"), fullPage: true });
  const digestText = await page.locator("p", { hasText: "Digest" }).first().innerText();
  const digest = /[0-9a-f]{64}/.exec(digestText)?.[0];
  check("release shows a full digest", !!digest);
  const releaseText = await page.locator('[role="tabpanel"]').innerText();
  check("release lists the fixed files and the held-back file count", releaseText.includes("finding-1.txt") && releaseText.includes("run-check.py") && /1 withheld file/.test(releaseText));
  check("release states what it does not establish", /does not establish/i.test(releaseText) && /private source repository/.test(releaseText));
  check("no secret value appears in the release", !/hunter2|sk_live/.test(releaseText));

  await page.getByRole("tab", { name: "Source" }).click();
  await page.getByRole("button", { name: /\.env/ }).click();
  await page.getByText("This file is labelled restricted").waitFor();
  const srcText = await page.locator(".viewer").innerText();
  check("withheld file is shown as withheld, never its content", /Withheld/.test(srcText) && !/hunter2|sk_live/.test(srcText));
  await page.getByRole("button", { name: /payments\/auth\/session\.py/ }).click();
  await page.getByText("def logout").waitFor();
  await page.screenshot({ path: join(shots, "03-source.png") });

  await page.getByRole("tab", { name: "Findings" }).click();
  await page.getByText("Confirmed by the kernel").waitFor();
  check("finding is confirmed by the kernel", true);

  await page.getByRole("tab", { name: "Protection" }).click();
  await page.getByText("possible prompt injection").first().waitFor();
  await page.getByText("refused: UnknownHandle").first().waitFor({ timeout: 15000 });
  const prot = await page.locator('[role="tabpanel"]').innerText();
  check("protection lists withheld files, injection suspects and refused requests", prot.includes(".env") && prot.includes("README.md") && /refused: UnknownHandle/.test(prot), prot.includes("refused") ? "" : "no refused rows");
  await page.screenshot({ path: join(shots, "04-protection.png"), fullPage: true });

  await page.getByRole("tab", { name: "Release" }).click();
  const noCapsuleYet = await page.getByRole("button", { name: "Sign and deliver the capsule" }).count();
  check("cannot deliver before approval", noCapsuleYet === 0);
  await page.getByRole("button", { name: "Request approval" }).click();
  await page.getByText("Waiting for you.").waitFor();
  const cmd = await page.locator("pre.cmd").innerText();
  check("page tells the user to approve in the terminal", /kaku approve appr-\d+/.test(cmd));
  await page.screenshot({ path: join(shots, "05-waiting.png"), fullPage: true });

  const approve = spawnSync(kaku, ["approve", "--data-dir", data], { input: digest.slice(0, 12) + "\n", encoding: "utf8" });
  check("terminal shows the exact files and accepts the digest prefix", approve.status === 0 && /RELEASE FOR APPROVAL/.test(approve.stdout) && /Approved\./.test(approve.stdout), approve.stderr.slice(0, 200));
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).waitFor({ timeout: 15000 });
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).click();
  await page.getByText("Delivered.").waitFor();
  await page.screenshot({ path: join(shots, "06-delivered.png"), fullPage: true });
  const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("link", { name: "Download the capsule" }).click()]);
  const capsulePath = join(work, "review.kkg");
  await download.saveAs(capsulePath);
  const cli = spawnSync(kaku, ["verify", capsulePath], { encoding: "utf8" });
  check("kaku verify accepts the delivered capsule", cli.status === 0 && /^VALID/m.test(cli.stdout) && /Not established/.test(cli.stdout));

  const v = await ctx.newPage();
  v.on("console", (m) => m.type() === "error" && problems.push("verify console: " + m.text()));
  v.on("pageerror", (e) => problems.push("verify pageerror: " + e.message));
  await v.goto(base + "/verify");
  await v.getByRole("heading", { name: "Check a capsule" }).waitFor();
  await v.setInputFiles("#file", capsulePath);
  await v.getByRole("heading", { name: "Valid" }).waitFor();
  check("the in-browser checker accepts it and lists what is not established", /Not established/.test(await v.locator("main").innerText()));
  await v.screenshot({ path: join(shots, "07-verify-valid.png"), fullPage: true });
  const bytes = readFileSync(capsulePath);
  const mark = Buffer.from('"repro-1.py":"');
  const at = bytes.lastIndexOf(mark) + mark.length;
  const tampered = Buffer.from(bytes);
  tampered[at] = tampered[at] === 65 ? 66 : 65;
  const tamperedPath = join(work, "tampered.kkg");
  writeFileSync(tamperedPath, tampered);
  await v.setInputFiles("#file", tamperedPath);
  await v.getByRole("heading", { name: "Not valid" }).waitFor();
  check("the in-browser checker rejects a tampered capsule with a reason", /changed|does not match/i.test(await v.locator("main").innerText()));
  await v.screenshot({ path: join(shots, "08-verify-tampered.png"), fullPage: true });
  const cliBad = spawnSync(kaku, ["verify", tamperedPath], { encoding: "utf8" });
  check("kaku verify rejects the tampered capsule", cliBad.status === 1 && /INVALID/.test(cliBad.stdout));

  const mobile = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "dark" });
  const mp = await mobile.newPage();
  await mp.goto(base + "/verify");
  await mp.getByRole("heading", { name: "Check a capsule" }).waitFor();
  await mp.screenshot({ path: join(shots, "09-mobile-dark-verify.png"), fullPage: true });
  const overflow = await mp.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
  check("verify page has no horizontal scroll on a phone", !overflow);
  await mobile.close();
} catch (e) {
  check("journey completed", false, String(e).slice(0, 400));
  await page.screenshot({ path: join(shots, "failure.png"), fullPage: true }).catch(() => {});
}
check("zero console errors, page errors, failed requests or 5xx responses", problems.length === 0, problems.join(" | ").slice(0, 400));
await browser.close();
server.kill();
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} checks passed`);
process.exit(failed ? 1 : 0);
