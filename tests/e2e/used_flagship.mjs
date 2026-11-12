// The flagship workflow as a person uses it, with a real model and the real sandbox (not part of the scripted suite: it
// takes minutes and its words differ run to run). A review of the sample project is started in the browser, read, approved
// in the terminal, delivered, downloaded and checked twice (command and web page); then a question is asked about an
// attached contract. Screenshots are saved for reading.
//
//   node tests/e2e/used_flagship.mjs [MODEL]        default qwen2.5-coder:7b; writes target/used-flagship/*.png
import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { cpSync, mkdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const model = process.argv[2] ?? "qwen2.5-coder:7b";
const work = join(repo, "target/used-flagship");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const project = join(root, "payments-service");
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
mkdirSync(join(root, "contracts"), { recursive: true });
const contract = join(root, "contracts/agreement.md");
writeFileSync(contract, "Master services agreement between Northwind and Acme.\nThe term of this agreement is twelve (12) months from the Effective Date.\nEither party may terminate with sixty (60) days written notice.\nInvoices are payable within thirty (30) days.\n");
const data = join(work, "data");
const kaku = join(repo, "target/debug/kaku");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
let passed = 0;
let failed = 0;
const check = (name, ok, detail = "") => {
  ok ? passed++ : failed++;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${!ok && detail ? `  ${detail}` : ""}`);
};
const server = spawn(kaku, ["serve", "--data-dir", data, "--root", root, "--model", model, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const until = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < until) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const base = new URL(link).origin;
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, acceptDownloads: true });
const page = await ctx.newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text().slice(0, 200)); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => { if (r.failure()?.errorText !== "net::ERR_ABORTED") problems.push(`requestfailed: ${r.url()} ${r.failure()?.errorText}`); });
page.on("response", (r) => { if (r.status() >= 500) problems.push(`status ${r.status()}: ${r.url()}`); });
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
// the part of the page a person sees without scrolling, for the README
const view = async (n) => { await page.evaluate(() => window.scrollTo(0, 0)); await page.screenshot({ path: join(work, `view-${n}.png`) }); };
const t0 = Date.now();
const secs = () => Math.round((Date.now() - t0) / 1000);
try {
  await page.goto(link);
  await page.waitForSelector("nav", { timeout: 30000 });
  const skip = page.getByRole("button", { name: "Skip the rest" });
  if (await skip.count()) await skip.click();
  const badges = await page.locator(".badges").innerText();
  check("the header says the model runs on this computer and names it", /Private: model runs on this computer/.test(badges) && badges.includes(model), badges);

  // ---- a review, start to delivered capsule ----
  await page.getByRole("button", { name: "payments-service" }).click();
  await page.waitForFunction(() => document.querySelector(".path")?.textContent?.endsWith("payments-service"));
  await page.getByLabel("Recipient name").fill("Acme audit");
  await page.getByRole("button", { name: "Start review" }).click();
  await page.waitForFunction(() => /^#\/job\//.test(location.hash), null, { timeout: 30000 });
  const job = await page.evaluate(() => location.hash.split("/")[2]);
  await shot("1-review-running");
  // the Stop button is there while the review works and goes when it has ended
  await page.getByRole("button", { name: "Stop", exact: true }).waitFor({ timeout: 30000 });
  await page.getByRole("button", { name: "Stop", exact: true }).waitFor({ state: "detached", timeout: 900000 });
  await page.getByRole("tab", { name: "Findings" }).click();
  console.log(`review ready after ${secs()} s`);
  await shot("2-review-ready");
  const main = await page.locator("main").innerText();
  check("the review finished with at least one finding the sandbox reproduced", /reproduced|confirmed/i.test(main), main.slice(0, 300));
  await page.getByRole("tab", { name: "Protection" }).click();
  const prot = await page.locator('[role="tabpanel"]').innerText();
  check("the protection tab says the .env file was withheld from the model", prot.includes(".env"), prot.slice(0, 300));
  await shot("3-protection");
  await view("protection");
  await page.getByRole("tab", { name: "Release" }).click();
  const digestLine = page.locator("p", { hasText: "Digest" }).first();
  await digestLine.waitFor({ timeout: 20000 });
  const digest = /[0-9a-f]{64}/.exec(await digestLine.innerText())?.[0] ?? "";
  check("the release shows its digest before anything is asked", digest.length === 64);
  await shot("4-release");
  await view("release");
  await page.getByRole("button", { name: "Request approval" }).click();
  await page.getByText("Waiting for you.").waitFor({ timeout: 20000 });
  const approve = spawnSync(kaku, ["approve", "--data-dir", data], { input: digest.slice(0, 12) + "\n", encoding: "utf8" });
  check("the terminal shows the exact release and accepts the digest's first characters", approve.status === 0 && /Approved\./.test(approve.stdout), approve.stdout.slice(-300) + approve.stderr.slice(0, 200));
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).waitFor({ timeout: 20000 });
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).click();
  const linkEl = page.getByRole("link", { name: "Download the capsule" });
  await linkEl.waitFor({ timeout: 30000 });
  const [download] = await Promise.all([page.waitForEvent("download"), linkEl.click()]);
  const capsule = join(work, "review.kkg");
  await download.saveAs(capsule);
  await shot("5-delivered");
  const cli = spawnSync(kaku, ["verify", capsule], { encoding: "utf8" });
  check("kaku verify accepts the capsule and says what it does not establish", cli.status === 0 && /^VALID/m.test(cli.stdout) && /Not established/.test(cli.stdout), cli.stdout.slice(0, 300));
  const v = await ctx.newPage();
  await v.goto(base + "/verify");
  await v.setInputFiles("#file", capsule);
  await v.getByRole("heading", { name: "Valid" }).waitFor({ timeout: 20000 });
  await v.screenshot({ path: join(work, "6-verify.png"), fullPage: true });
  await v.screenshot({ path: join(work, "view-verify.png") });
  check("the recipient's web page accepts the same capsule", /Not established/.test(await v.locator("main").innerText()));
  await v.close();
  const out = await page.evaluate(async () => (await (await fetch("/api/outbound")).json()));
  check("the list of what left this computer names the release and nothing else", (out.rows ?? []).length === 1 && out.rows[0].what === "release" && out.totals?.model === 0, JSON.stringify(out).slice(0, 300));

  // ---- a question about an attached contract ----
  await page.goto(base + "/#/chat");
  await page.getByLabel("Your message").waitFor();
  await page.locator('input[type="file"][aria-label="Attach files"]').setInputFiles(contract);
  await page.getByText("agreement.md").first().waitFor({ timeout: 20000 });
  await page.getByLabel("Your message").fill("How many days of notice does either party need to give to terminate? Answer in one sentence and say where it is written.");
  const asked = Date.now();
  await page.getByRole("button", { name: "Send", exact: true }).click();
  // the reply is over when Stop can no longer be pressed
  await page.getByRole("button", { name: "Stop", exact: true }).waitFor({ timeout: 30000 }).catch(() => {});
  await page.waitForFunction(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent?.trim() === "Stop"); return document.querySelectorAll(".msg").length >= 2 && (!b || b.disabled); }, null, { timeout: 240000 });
  const answer = await page.locator(".msg").last().innerText();
  console.log(`chat answered after ${Math.round((Date.now() - asked) / 1000)} s`);
  check("the model answers the contract question correctly from the attached file", /\b(60|sixty)\b/i.test(answer), answer.slice(0, 300));
  console.log("answer: " + answer.replace(/\s+/g, " ").slice(0, 400));
  await shot("7-chat-answer");
  await page.locator(".msg").last().scrollIntoViewIfNeeded();
  await page.screenshot({ path: join(work, "view-chat.png") });
  check("no console errors, page errors, failed requests or 5xx responses", problems.length === 0, problems.slice(0, 4).join(" | "));
} catch (e) {
  check("the walk finished without an error", false, String(e?.stack ?? e).slice(0, 500));
  await shot("error").catch(() => {});
}
await browser.close();
server.kill();
console.log(`\n${passed}/${passed + failed} checks passed in ${secs()} s`);
process.exit(failed ? 1 : 0);
