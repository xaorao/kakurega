// The standalone capsule check page, opened from disk with no server and no network: a valid capsule, a forged model
// receipt, a changed byte, a file that is not a capsule, a wrong and a right pinned key, a phone-width view and dark mode.
//
//   node tests/e2e/verifier_page.mjs     screenshots in target/verifier-page/

import { chromium } from "playwright-core";
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/verifier-page");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
const run = (cmd, args, cwd) => {
  const r = spawnSync(cmd, args, { cwd, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(" ")}: ${r.stderr}`);
};
run("node", ["build.mjs"], join(repo, "apps/verifier-page"));
run("cargo", ["run", "-q", "-p", "kk-evidence", "--example", "make_receipt_capsules", "--", work], repo);
const page_url = "file://" + join(repo, "apps/verifier-page/dist/kakurega-verify.html");
const two = join(work, "two.kkg");
const bytes = readFileSync(two);
const flipped = Buffer.from(bytes);
flipped[flipped.length - 20] ^= 1;
writeFileSync(join(work, "flipped.kkg"), flipped);
writeFileSync(join(work, "notes.txt"), "just some text\n");
const key = JSON.parse(bytes.toString()).keys[0];

const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1000, height: 900 } });
const problems = [];
const requests = [];
const page = await ctx.newPage();
page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("request", (r) => { if (!r.url().startsWith("file:")) requests.push(r.url()); });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const verdict = async () => (await page.locator(".verdict").first().innerText()).trim();

await page.goto(page_url);
await page.setInputFiles("#file", two);
await page.locator(".verdict").waitFor();
check((await verdict()).startsWith("Valid"), "a good capsule is valid");
let body = await page.locator("main").innerText();
check(/Model signatures checked \(2\)/.test(body) && /vendor\/model/.test(body), "and both model signatures are listed");
check(/does not establish/i.test(body) && /attested confidential hardware/.test(body) && /answer is true/.test(body), "and it says what the check does not establish, hardware and truth included");
check(/no, this is only the key the capsule carries/.test(body), "and it says the signer is not pinned");
await page.screenshot({ path: join(work, "valid.png"), fullPage: true });

await page.setInputFiles("#file", join(work, "forged.kkg"));
await page.getByText("model receipt's signature does not match").waitFor();
check((await verdict()) === "Not valid", "a forged model receipt is refused, with the reason in words");
await page.screenshot({ path: join(work, "forged.png"), fullPage: true });

await page.setInputFiles("#file", join(work, "flipped.kkg"));
await page.locator(".verdict").waitFor();
check((await verdict()) === "Not valid", "a capsule with one changed byte is refused");

await page.setInputFiles("#file", join(work, "notes.txt"));
await page.getByText("The file is not a capsule").waitFor();
check(true, "a text file is refused as not a capsule");

await page.setInputFiles("#file", two);
await page.locator(".verdict").waitFor();
await page.fill("#pin", "A".repeat(43));
await page.getByText("Not valid").waitFor();
check((await verdict()) === "Not valid", "a capsule is refused when the expected key is a different key");
await page.fill("#pin", "not a key");
await page.getByText("not 43 base64url characters").waitFor();
check(true, "a malformed expected key is reported, not ignored");
await page.fill("#pin", key);
await page.getByText("matches the key you pasted").waitFor();
check((await verdict()).startsWith("Valid"), "the right expected key gives valid and says it is pinned");

await page.fill("#pin", "");
const late = await ctx.newPage();
await late.clock.setFixedTime(new Date("2035-01-01T00:00:00Z"));
await late.goto(page_url);
await late.setInputFiles("#file", two);
await late.locator(".verdict").waitFor();
check((await late.locator(".verdict").innerText()).trim() === "Not valid" && /expired/i.test(await late.locator("main").innerText()), "a capsule is refused as expired when the device clock is past its expiry");

check(requests.length === 0, "no network request was made: " + requests.join(", "));
check(problems.length === 0, "no console errors: " + problems.join("; "));

const phone = await (await browser.newContext({ viewport: { width: 375, height: 800 }, colorScheme: "dark" })).newPage();
await phone.goto(page_url);
await phone.setInputFiles("#file", two);
await phone.locator(".verdict").waitFor();
const overflow = await phone.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
check(!overflow, "phone width in dark mode has no sideways scroll");
await phone.screenshot({ path: join(work, "phone-dark.png"), fullPage: true });
await browser.close();
