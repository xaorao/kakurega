// The sample as a new person uses it: load it from the first screen, ask its documents a question, take notes from its
// meeting, pick its project for a review, remove it. The model is the adversary of the leak benchmark, so it also shows
// that the made-up key and password in the sample never reach a model.
//   node tests/e2e/sample_web.mjs      screenshots in target/sample-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/sample-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 26000 + (process.pid % 1000);
const procs = [];
process.on("exit", () => { for (const p of procs) { try { p.kill(); } catch {} } });
writeFileSync(join(work, "canaries.json"), JSON.stringify({ needles: ["sk-ant-api03-SAMPLE", "hunter2"] }));
procs.push(spawn("python3", [join(repo, "benchmarks/leak/adversary_model.py"), "127.0.0.1", String(port), join(work, "canaries.json"), join(work, "model.log")], { stdio: "ignore" }));
await new Promise((r) => setTimeout(r, 600));
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", "adversary", "--ollama", `http://127.0.0.1:${port}`, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
procs.push(server);
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1300 } })).newPage();
const errors = [];
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
page.on("response", (r) => { if (r.status() >= 400) errors.push(`${r.status()} ${r.request().method()} ${new URL(r.url()).pathname}${new URL(r.url()).search}`); });
page.on("pageerror", (e) => errors.push(String(e)));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.waitForSelector("#sample");
await page.getByRole("button", { name: "Load the sample" }).click();
await page.getByRole("button", { name: "Ask the sample documents" }).waitFor();
const dir = join(root, "kakurega-sample");
check(existsSync(join(dir, "payments-service/payments/auth/session.py")) && existsSync(join(dir, "documents/supply-agreement.md")), "the sample files are in a folder of their own inside the allowed folder");
check((await page.locator("#sample").innerText()).includes("made up"), "the card says everything in the sample is made up");
await page.screenshot({ path: join(work, "1-loaded.png"), fullPage: true });

await page.getByRole("button", { name: "Ask the sample documents" }).click();
await page.waitForSelector("#h-docs");
await page.getByText("kakurega-sample/documents").first().waitFor();
check(true, "the Documents screen opens on the sample's documents folder");

await page.goto(new URL(link).origin + "/#/");
await page.waitForSelector("#sample");
await page.getByRole("button", { name: "Notes from the sample meeting" }).click();
await page.waitForSelector("#meet-text");
check((await page.inputValue("#meet-text")).startsWith("Asha Rao: Thanks everyone.") && (await page.inputValue("#meet-title")) === "Weekly sync (sample)", "the Meeting notes screen opens with the sample transcript and its name");
await page.getByRole("button", { name: "Write the notes" }).click();
await page.waitForSelector("#meet-result");
check((await page.locator("#meet-result").innerText()).includes("1 line that holds a secret was not sent"), "the line with the made-up password is held back, and the screen says so");

await page.goto(new URL(link).origin + "/#/");
await page.waitForSelector("#sample");
await page.getByRole("button", { name: "Choose the sample project for a review" }).click();
await page.getByText("kakurega-sample/payments-service").first().waitFor();
check(true, "the sample project is chosen as the folder for a review");

const modelLog = readFileSync(join(work, "model.log"), "utf8");
check(modelLog.includes("Thanks everyone") && !modelLog.includes("hunter2") && !modelLog.includes("sk-ant-api03-SAMPLE"), "the model's own log holds the meeting and neither the made-up password nor the made-up key");

writeFileSync(join(dir, "documents/my-own.txt"), "mine");
await page.getByRole("button", { name: "Remove the sample" }).click();
await page.getByRole("button", { name: "Load the sample" }).waitFor();
check(!existsSync(join(dir, "payments-service")) && readFileSync(join(dir, "documents/my-own.txt"), "utf8") === "mine", "removing takes the sample's files and leaves a file the person added");
check(errors.length === 0, "no console errors" + (errors.length ? ": " + errors.join(" | ") : ""));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
