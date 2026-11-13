// Choosing where the model runs, as a person does it in Settings: local model first, then NEAR AI Cloud with a model
// name and an account key, then back. The first key is made up, so nothing can be answered by NEAR; what is checked is the
// switch itself, where the key ends up, what is shown, and that the choice is kept for the next start. With a real account
// key (KAKU_TEST_NEAR_KEY_FILE, a file of NEAR_AI_API_KEY=... lines, and KAKU_TEST_NEAR_MODEL), the real key is then typed
// into Settings the same way, a question is asked in Chat and answered from NEAR's confidential hardware, and the key is
// forgotten again at the end.
//   node tests/e2e/route_web.mjs      screenshots in target/route-web/

import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/route-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
const data = join(work, "data");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 23000 + (process.pid % 1000);
const procs = [];
process.on("exit", () => { for (const p of procs) { try { p.kill(); } catch {} } });
procs.push(spawn("python3", [join(repo, "tests/e2e/team_script.py"), String(port)], { stdio: "ignore" }));
await new Promise((r) => setTimeout(r, 600));
const KEY = ["sk", "test", "route", "0123456789abcdefghij"].join("-");

async function serve() {
  const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", data, "--root", join(work, "root"), "--model", "team-script", "--ollama", `http://127.0.0.1:${port}`, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
  procs.push(server);
  let log = "";
  server.stdout.on("data", (d) => (log += d));
  server.stderr.on("data", (d) => (log += d));
  const deadline = Date.now() + 30000;
  while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
  const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
  if (!link) throw new Error("no pairing link:\n" + log);
  return { server, link, log: () => log };
}
const filesHolding = (dir, needle) => {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...filesHolding(p, needle));
    else if (readFileSync(p).includes(needle)) out.push(p.slice(data.length + 1));
  }
  return out;
};

let { server, link, log } = await serve();
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1200 } });
let page = await ctx.newPage();
const errors = [];
const watch = (p) => { p.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); }); p.on("pageerror", (e) => errors.push(String(e))); };
watch(page);
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const open = async (l) => {
  await page.goto(l);
  await page.waitForSelector("nav");
  const skip = page.getByRole("button", { name: "Skip the rest" });
  if (await skip.count()) await skip.click();
  await page.goto(new URL(l).origin + "/#/settings");
  await page.waitForSelector("#route-current");
};

await open(link);
check((await page.locator("#route-current").innerText()).includes("the model team-script on this computer"), "it starts on the model on this computer");
check(await page.getByRole("button", { name: "Save and use NEAR AI Cloud" }).isDisabled(), "NEAR cannot be chosen without a model and a key");
await page.fill("#route-model", "deepseek-ai/DeepSeek-V3.1");
await page.fill("#route-key", "has spaces in it");
await page.getByRole("button", { name: "Save and use NEAR AI Cloud" }).click();
await page.locator("#route-choice").getByRole("alert").waitFor();
check((await page.locator("#route-choice").getByRole("alert").innerText()).includes("not a plain account key") && (await page.locator("#route-current").innerText()).includes("on this computer"), "a key that is not a key is refused in words and nothing changes");
await page.fill("#route-key", KEY);
await page.getByRole("button", { name: "Save and use NEAR AI Cloud" }).click();
await page.getByText("NEAR AI Cloud answers from now on").waitFor();
const now = await page.locator("#route-current").innerText();
check(now.includes("deepseek-ai/DeepSeek-V3.1 on NEAR AI Cloud") && now.includes("near.ai"), "the route in use is NEAR, with its model and host");
check((await page.inputValue("#route-key")) === "", "the key box is empty again");
await page.waitForFunction(() => !document.querySelector("header")?.textContent?.includes("model runs on this computer"));
check(!(await page.locator("header").innerText()).includes("model runs on this computer"), "the header no longer says the model runs on this computer");
check(!(await page.content()).includes(KEY), "the key is nowhere in the page");
await page.getByText("In use now: the model that answers is not on this computer.").waitFor();
check(true, "the names-to-hide setting on the same page now says it is in use");
const box = await page.evaluate(() => { const k = document.querySelector("#route-key").getBoundingClientRect(); const m = document.querySelector("#route-model").getBoundingClientRect(); return [k.height, m.height]; });
check(Math.abs(box[0] - box[1]) < 2, "the key box looks like the other boxes");
await page.screenshot({ path: join(work, "1-near.png"), fullPage: true });

const holding = filesHolding(data, KEY);
check(holding.length === 1 && holding[0] === "near.key", "on disk the key is in one file only: " + holding.join(", "));
check((statSync(join(data, "near.key")).mode & 0o777) === 0o600, "that file can be read by its owner only");
check(!log().includes(KEY), "the key is not in the service's output");

// the choice is kept: a service started again with no flag about the route is on NEAR
await page.close(); // its live connection would report the service going away, which is the point of the restart
server.kill();
await new Promise((r) => setTimeout(r, 500));
({ server, link, log } = await serve());
page = await ctx.newPage();
watch(page);
await open(link);
check((await page.locator("#route-current").innerText()).includes("on NEAR AI Cloud"), "after a restart the saved choice is in use");
check(await page.getByRole("button", { name: "Forget the saved key" }).count() === 0, "the key cannot be forgotten while it is in use");

// back to this computer, and the model here answers
await page.getByRole("button", { name: "Use the model on this computer" }).click();
await page.getByText("The model on this computer answers from now on.").waitFor();
check((await page.locator("#route-current").innerText()).includes("on this computer"), "switched back to the model on this computer");
const reply = await page.evaluate(async () => {
  const csrf = (await (await fetch("/api/session", { credentials: "same-origin" })).json()).csrf;
  const r = await fetch("/api/chat", { method: "POST", headers: { "Content-Type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify({ message: "hello" }) });
  return { status: r.status, body: await r.json() };
});
check(reply.status === 200 && reply.body.route.stays_here === true, "a question is answered by the model on this computer");

const nearModel = process.env.KAKU_TEST_NEAR_MODEL;
const keyFile = process.env.KAKU_TEST_NEAR_KEY_FILE;
if (nearModel && keyFile) {
  // the real key, typed into Settings as a person pastes it; it is never printed
  const realKey = readFileSync(keyFile, "utf8").split("\n").map((l) => l.trim()).find((l) => l.startsWith("NEAR_AI_API_KEY="))?.slice("NEAR_AI_API_KEY=".length) ?? "";
  check(realKey.length > 10, "a real account key was read from the key file");
  await page.fill("#route-model", nearModel);
  await page.fill("#route-key", realKey);
  await page.getByRole("button", { name: "Save and use NEAR AI Cloud" }).click();
  await page.getByText("NEAR AI Cloud answers from now on").waitFor({ timeout: 60000 });
  check((await page.locator("#route-current").innerText()).includes(`${nearModel} on NEAR AI Cloud`), "with the real key the route in use is NEAR, with the model named");
  await page.getByRole("link", { name: "Chat", exact: true }).click();
  await page.getByLabel("Your message").waitFor();
  await page.getByLabel("Your message").fill("In one short sentence: what is a session token?");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await page.getByRole("button", { name: "Stop", exact: true }).waitFor({ timeout: 30000 }).catch(() => {});
  await page.waitForFunction(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent?.trim() === "Stop"); return document.querySelectorAll(".msg").length >= 2 && (!b || b.disabled); }, null, { timeout: 240000 });
  const answer = await page.locator(".msg").last().innerText();
  check(/token/i.test(answer) && answer.length > 20, "the question is answered: " + answer.replace(/\s+/g, " ").slice(0, 160));
  check(await page.locator("#badge-tee").isVisible(), "the header says the answer comes from NEAR AI's confidential hardware");
  // the model's own signature over this answer is under it, and checks out with nothing but the key and the text signed
  const line = page.locator("details.receipt").last();
  await line.waitFor({ timeout: 10000 });
  check(/signed by the model's hardware/.test(await line.locator("summary").innerText()), "the answer says it was signed by the model's hardware");
  await line.locator("summary").click();
  const facts = await line.locator("dl").innerText();
  const signerKey = /Signing key\s+([0-9a-f]{64})/.exec(facts)?.[1] ?? "";
  const sigHex = /Signature\s+([0-9a-f]{128})/.exec(facts)?.[1] ?? "";
  const reqHash = /Question fingerprint \(SHA-256\)\s+([0-9a-f]{64})/.exec(facts)?.[1] ?? "";
  const ansHash = /Answer fingerprint \(SHA-256\)\s+([0-9a-f]{64})/.exec(facts)?.[1] ?? "";
  const { createPublicKey, verify: edVerify } = await import("node:crypto");
  const spki = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), Buffer.from(signerKey, "hex")]);
  const signedText = `${nearModel}:${reqHash}:${ansHash}`;
  let sigOk = false;
  try { sigOk = edVerify(null, Buffer.from(signedText), createPublicKey({ key: spki, format: "der", type: "spki" }), Buffer.from(sigHex, "hex")); } catch {}
  check(sigOk, "the signature shown verifies with Node's own Ed25519 against the key and the two fingerprints shown: key " + signerKey.slice(0, 12));
  let forged = false;
  try { forged = edVerify(null, Buffer.from(signedText.replace(/.$/, (c) => (c === "0" ? "1" : "0"))), createPublicKey({ key: spki, format: "der", type: "spki" }), Buffer.from(sigHex, "hex")); } catch {}
  check(!forged, "and does not verify for a changed answer fingerprint");
  await page.screenshot({ path: join(work, "near-receipt.png"), fullPage: true });
  await page.screenshot({ path: join(work, "3-near-answer.png"), fullPage: true });
  const where = await page.evaluate(async () => {
    const r = await fetch("/api/usage", { credentials: "same-origin" });
    return await r.json();
  });
  check(where.routes.some((r) => r.route === `near:${nearModel}` && r.class === "verified_tee" && r.turns >= 1), "the usage record counts the reply on NEAR");
  check(!(await page.content()).includes(realKey) && !log().includes(realKey), "the real key is in neither the page nor the service's output");
  const realHolding = filesHolding(data, realKey);
  check(realHolding.length === 1 && realHolding[0] === "near.key", "on disk the real key is in one file only: " + realHolding.join(", "));
  await page.goto(new URL(link).origin + "/#/settings");
  await page.waitForSelector("#route-current");
  await page.getByRole("button", { name: "Use the model on this computer" }).click();
  await page.getByText("The model on this computer answers from now on.").waitFor();
}
await page.getByRole("button", { name: "Forget the saved key" }).click();
await page.getByText("The saved key was removed from this computer.").waitFor();
check(!existsSync(join(data, "near.key")) && filesHolding(data, KEY).length === 0, "the key is gone from this computer");
check(await page.getByRole("button", { name: "Save and use NEAR AI Cloud" }).isDisabled(), "NEAR needs the key again");
await page.screenshot({ path: join(work, "2-local.png"), fullPage: true });
const other = errors.filter((e) => !/status of 422/.test(e));
check(other.length === 0, "no console errors beyond the one refusal" + (other.length ? ": " + other.join(" | ") : ""));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
