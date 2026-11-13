// The pictures on the public site, taken from the real product with real models: the start screen (model on this
// computer), a release waiting for approval (a finished review's data folder), and a chat answer from NEAR AI Cloud with
// its signed receipt open. Run by hand; needs Ollama, a finished review and a NEAR key. Costs one short answer on NEAR.
//   KAKU_TEST_NEAR_KEY_FILE=... node tests/e2e/site_shots.mjs REVIEW_DATA_DIR REVIEW_ROOT      writes apps/site/img/
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const out = join(repo, "apps/site/img");
const [reviewData, reviewRoot] = process.argv.slice(2);
const keyFile = process.env.KAKU_TEST_NEAR_KEY_FILE;
if (!reviewData || !reviewRoot || !keyFile) throw new Error("usage: KAKU_TEST_NEAR_KEY_FILE=FILE node tests/e2e/site_shots.mjs REVIEW_DATA_DIR REVIEW_ROOT");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });

async function withService(args, data, fn) {
  const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", data, "--ui-dir", join(repo, "apps/web/dist"), ...args], { stdio: ["ignore", "pipe", "pipe"] });
  let log = "";
  server.stdout.on("data", (d) => (log += d));
  server.stderr.on("data", (d) => (log += d));
  try {
    const deadline = Date.now() + 60000;
    while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
    const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
    if (!link) throw new Error("the service did not start:\n" + log.slice(-600));
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 }, colorScheme: "light", deviceScaleFactor: 2 });
    const page = await ctx.newPage();
    await page.goto(link);
    await page.getByRole("button", { name: "Skip the rest" }).click({ timeout: 6000 }).catch(() => {});
    await fn(page, new URL(link).origin);
    await ctx.close();
  } finally {
    server.kill();
    await new Promise((r) => setTimeout(r, 800));
  }
}

const fresh = join(repo, "target/site-shots");
rmSync(fresh, { recursive: true, force: true });
mkdirSync(join(fresh, "root"), { recursive: true });

// 1. the start, with the model on this computer
await withService(["--root", join(fresh, "root")], join(fresh, "data-local"), async (page, origin) => {
  await page.goto(origin + "/#/");
  await page.locator("h1.start-hello").waitFor();
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(out, "start.png") });
  console.log("start.png");
});

// 2. a real review's release, waiting for the owner
await withService(["--root", reviewRoot], reviewData, async (page, origin) => {
  await page.goto(origin + "/#/job/job-1");
  await page.getByText("Release", { exact: true }).first().click();
  await page.getByText("change.txt").first().waitFor({ timeout: 20000 });
  await page.waitForTimeout(400);
  await page.screenshot({ path: join(out, "release.png") });
  console.log("release.png");
});

// 3. an answer from NEAR AI Cloud with its receipt open
await withService(["--root", join(fresh, "root"), "--route", "near", "--near-model", process.env.KAKU_TEST_NEAR_MODEL ?? "Qwen/Qwen3.8-27B", "--near-key-file", keyFile], join(fresh, "data-near"), async (page, origin) => {
  await page.goto(origin + "/#/chat");
  await page.getByLabel("Your message").fill("In one sentence: why should a session token be removed on the server at logout?");
  await page.getByLabel("Your message").press("Enter");
  const line = page.locator("details.receipt").last();
  await line.waitFor({ timeout: 120000 });
  await line.locator("summary").click();
  await line.scrollIntoViewIfNeeded();
  await page.evaluate(() => { const d = document.querySelector("details.receipt"); window.scrollTo(0, Math.max(0, d.getBoundingClientRect().top + scrollY - 330)); });
  await page.waitForTimeout(500);
  const text = await page.content();
  const key = readFileSync(keyFile, "utf8").match(/NEAR_AI_API_KEY=(\S+)/)?.[1] ?? "";
  if (key && text.includes(key)) throw new Error("the key is in the page; no picture taken");
  await page.screenshot({ path: join(out, "receipt.png") });
  console.log("receipt.png");
});
await browser.close();
