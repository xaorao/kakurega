// The web Code screen as a person uses it, with a real local model: choose a folder, describe the change, read the patch,
// apply it after confirming, check the file on disk, undo, check again. Needs Ollama with the model given (default
// qwen2.5-coder:7b) and the sandbox machine running.
//
//   node tests/e2e/code_real.mjs [MODEL]     screenshots in target/code-real/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
import { createHash } from "node:crypto";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/code-real");
const model = process.argv[2] ?? "qwen2.5-coder:7b";
// KAKU_TEST_NEAR_MODEL and KAKU_TEST_NEAR_KEY_FILE run the model work on NEAR AI Cloud's confidential hardware instead
const nearModel = process.env.KAKU_TEST_NEAR_MODEL;
const near = nearModel ? ["--route", "near", "--near-model", nearModel, "--near-key-file", process.env.KAKU_TEST_NEAR_KEY_FILE ?? ""] : null;

rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const proj = join(root, "shop");
mkdirSync(proj, { recursive: true });
const original = "def apply_discount(price, percent):\n    \"\"\"Return the price after taking `percent` percent off.\"\"\"\n    return price - percent\n\n\ndef total(items):\n    \"\"\"Sum of price * quantity for (price, quantity) pairs.\"\"\"\n    return sum(p for p, q in items)\n";
writeFileSync(join(proj, "pricing.py"), original);
writeFileSync(join(proj, "test_pricing.py"), "from pricing import apply_discount, total\n\n\ndef test_discount():\n    assert apply_discount(200, 10) == 180\n\n\ndef test_total():\n    assert total([(2, 3), (5, 1)]) == 11\n");
const sha = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
const before = sha(join(proj, "pricing.py"));
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", model, ...(near ?? []), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
process.on("exit", () => { try { server.kill(); } catch {} });
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const page = await ctx.newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => { if (!/ERR_ABORTED/.test(r.failure()?.errorText ?? "")) problems.push("requestfailed: " + r.url()); });
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { if (!cond) { console.log("FAIL", what); process.exitCode = 1; } else console.log("PASS", what); };

await page.goto(link);
await page.waitForSelector("nav");
await page.goto(new URL(link).origin + "/#/code");
await page.getByRole("heading", { name: "Change a project" }).waitFor();
await page.getByRole("button", { name: "shop" }).click();
await page.locator(".path").filter({ hasText: "/shop" }).waitFor();
await page.getByLabel("What should change").fill("The tests fail. Fix pricing.py so both tests in test_pricing.py pass. Do not change the tests.");
await page.getByRole("button", { name: "Start" }).click();
await page.getByText("Working.").waitFor();
await shot("1-working");
await page.getByRole("heading", { name: /^Result/ }).waitFor();
await page.waitForFunction(() => !document.querySelector('[aria-busy="true"]'), null, { timeout: 900000 });
await shot("2-result");
const text = await page.locator("section[aria-labelledby=h-code-result]").innerText();
check(sha(join(proj, "pricing.py")) === before, "nothing on disk changed before apply");
const passed = /Tests pass on the changed files/.test(text);
check(passed, "the kernel's test run passes on the patch");
check(/pricing\.py/.test(text) && /return sum\(p \* q/.test(text), "the patch is shown with the changed line");
const stepsBox = page.locator("details.steps");
check((await stepsBox.count()) === 1 && /What the model did \(\d+ steps\)/.test(await stepsBox.locator("summary").innerText()), "the list of what the model did is there, with its step count");
await stepsBox.locator("summary").click();
const stepsText = await stepsBox.innerText();
await shot("2b-steps");
check(/Edited\s+pricing\.py/.test(stepsText) && /Read\s+pricing\.py|Listed|Ran the tests/.test(stepsText), "it names the files the model read and edited: " + stepsText.slice(0, 200).replace(/\n/g, " | "));
check(!/return sum\(p \* q|old_text|new_text/.test(stepsText), "and never shows the code it wrote");
const apply = page.getByRole("button", { name: "Apply this patch" });
if (await apply.count()) {
  await apply.click();
  await page.getByRole("alertdialog").waitFor();
  await shot("3-confirm");
  await page.getByRole("button", { name: "Cancel" }).click();
  check(sha(join(proj, "pricing.py")) === before, "cancel changes nothing");
  await page.getByRole("button", { name: "Apply this patch" }).click();
  await page.getByRole("button", { name: "Yes, apply it" }).click();
  await page.getByText("Applied.").waitFor();
  await shot("4-applied");
  check(sha(join(proj, "pricing.py")) !== before, "the file on disk changed after apply");
  check(/return sum\(p \* q/.test(readFileSync(join(proj, "pricing.py"), "utf8")), "the applied text is the patch shown");
  await page.getByRole("button", { name: "Undo" }).click();
  await page.getByText("Every file is back").waitFor();
  await shot("5-undone");
  check(sha(join(proj, "pricing.py")) === before, "undo restores the exact original bytes");
} else {
  check(false, "an apply button was offered");
}
check(problems.length === 0, "no console errors or failed requests: " + problems.join(" | "));
await browser.close();
server.kill();
process.exit(process.exitCode ?? 0);
