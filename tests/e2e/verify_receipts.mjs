// A recipient with no account checks a capsule that carries a model's signatures, in the command line and in the browser page:
// the receipts are listed, the page says what is and is not established, and a capsule with a forged receipt is refused with its reason.
//
//   node tests/e2e/verify_receipts.mjs     screenshots in target/verify-receipts/

import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/verify-receipts");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
const kaku = join(repo, "target/debug/kaku");
const made = spawnSync("cargo", ["run", "-q", "-p", "kk-evidence", "--example", "make_receipt_capsules", "--", work], { cwd: repo, encoding: "utf8" });
if (made.status !== 0) throw new Error("could not make the capsules: " + made.stderr);
const files = { two: [null, join(work, "two.kkg")], forged: [null, join(work, "forged.kkg")] };
const results = [];
const check = (cond, what) => { results.push(!!cond); console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
// the command line
const cli = (p) => spawnSync(kaku, ["verify", p], { encoding: "utf8" });
let r = cli(files.two[1]);
check(r.status === 0 && /^VALID/m.test(r.stdout), "kaku verify accepts a capsule with two model receipts");
check(/Model answers signed by their model \(2\)/.test(r.stdout) && /vendor\/model answer chat-1/.test(r.stdout), "and lists each answer with its hashes and signer");
check(/attested confidential hardware/.test(r.stdout), "and says what is not established about the hardware");
r = cli(files.forged[1]);
check(r.status !== 0 && /INVALID: receipt_signature/.test(r.stdout), "a capsule whose receipt text was changed after signing is refused with its reason");

// the browser page
const server = spawn(kaku, ["serve", "--data-dir", join(work, "data"), "--root", work, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
process.on("exit", () => { try { server.kill(); } catch {} });
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r2) => setTimeout(r2, 100));
const origin = new URL(/http:\/\/127\.0\.0\.1:\d+/.exec(log)[0]).origin;
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1100, height: 900 } })).newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
await page.goto(origin + "/verify");
await page.setInputFiles("#file", files.two[1]);
await page.getByRole("heading", { name: "Model answers signed by the model" }).waitFor();
await page.screenshot({ path: join(work, "valid.png"), fullPage: true });
const body = await page.locator("main").innerText();
check(/vendor\/model: answer chat-1/.test(body) && /vendor\/model: answer chat-2/.test(body), "the page lists both model answers");
check(/That a signer key belongs to attested confidential hardware/.test(body), "and says the hardware is not established offline");
await page.setInputFiles("#file", files.forged[1]);
await page.getByText("A model receipt's signature does not verify").waitFor();
await page.screenshot({ path: join(work, "forged.png"), fullPage: true });
check(true, "a forged receipt is refused on the page with its reason in words");
check(problems.length === 0, "no console errors: " + problems.join("; "));
await browser.close();
server.kill();
