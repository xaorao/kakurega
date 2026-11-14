// The screens the other real-use runs do not reach, used as a person uses them with a real local model (run by hand; it
// takes minutes and the model's words differ run to run): a note that is remembered, used in an answer, then forgotten;
// standing instructions; a file attached to a chat with a secret in it; a roundtable; a question to a project's code;
// a lease for an agent given and revoked; and what Activity and "Where your data goes" then say.
//
//   node tests/e2e/used_rest.mjs [MODEL]       default qwen2.5-coder:7b; writes target/used-rest/*.png
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { cpSync, mkdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const model = process.argv[2] ?? "qwen2.5-coder:7b";
// KAKU_TEST_NEAR_MODEL and KAKU_TEST_NEAR_KEY_FILE run the model work on NEAR AI Cloud's confidential hardware instead
const nearModel = process.env.KAKU_TEST_NEAR_MODEL;
const near = nearModel ? ["--route", "near", "--near-model", nearModel, "--near-key-file", process.env.KAKU_TEST_NEAR_KEY_FILE ?? ""] : [];
const work = join(repo, nearModel ? "target/used-rest-near" : "target/used-rest");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const project = join(root, "payments-service");
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
mkdirSync(join(root, "contracts"), { recursive: true });
writeFileSync(join(root, "contracts/agreement.md"), "Master services agreement between Northwind and Acme.\nThe term of this agreement is twelve (12) months from the Effective Date.\nEither party may terminate with sixty (60) days written notice.\nInvoices are payable within thirty (30) days.\n");
const made = ["sk-ant", "api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH"].join("-");
writeFileSync(join(root, "contracts/keys.txt"), `Billing API_KEY=${made}\n`);
const transcript = join(work, "weekly-sync.txt");
writeFileSync(transcript, ["Dana: We ship the new pricing page on Friday.", "Lee: I will write the migration script by Wednesday.", "Dana: The staging password is still hunter2x9, please rotate it.", "Lee: Open question: do we keep the old plan names?", "Dana: Agreed, refunds over 500 need a second approval."].join("\n") + "\n");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
let passed = 0;
let failed = 0;
const check = (name, ok, detail = "") => {
  ok ? passed++ : failed++;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${!ok && detail ? `  ${detail}` : ""}`);
};
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", model, ...near, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
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
const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const page = await ctx.newPage();
const problems = [];
// the one refusal this run asks for (a lease for zero uses) is answered 400, and the browser logs that
page.on("console", (m) => { if (m.type() === "error" && !(/status of 400/.test(m.text()) && /agent-access\/grant/.test(m.location()?.url ?? ""))) problems.push("console: " + m.text().slice(0, 200) + " " + (m.location()?.url ?? "")); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => { if (r.failure()?.errorText !== "net::ERR_ABORTED") problems.push(`requestfailed: ${r.url()} ${r.failure()?.errorText}`); });
page.on("response", (r) => { if (r.status() >= 500) problems.push(`status ${r.status()}: ${r.url()}`); });
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const t0 = Date.now();
const lap = (() => { let at = Date.now(); return () => { const s = Math.round((Date.now() - at) / 1000); at = Date.now(); return s; }; })();
const flat = (s) => s.replace(/\s+/g, " ");
const step = async (name, fn) => {
  try {
    await fn();
  } catch (e) {
    check(`${name}: the step finished`, false, String(e?.message ?? e).split("\n")[0].slice(0, 300));
    await shot(`error-${name}`).catch(() => {});
  }
};


await page.goto(link);
await page.waitForSelector("nav", { timeout: 30000 });
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
lap();
const main = async () => flat(await page.locator("main").innerText());
const answered = async (n) => {
  await page.waitForFunction((k) => document.querySelectorAll(".msg.assistant").length >= k, n, { timeout: 240000 });
  await page.waitForFunction(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent?.trim() === "Stop"); return !b || b.disabled; }, null, { timeout: 240000 });
  return flat(await page.locator(".msg.assistant").nth(n - 1).innerText());
};
const ask = async (text, n) => {
  await page.getByLabel("Your message").fill(text);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  return answered(n);
};
const freshChat = async () => {
  await page.goto(base + "/#/chat");
  await page.getByLabel("Your message").waitFor();
  const clear = page.getByRole("button", { name: "Clear", exact: true });
  if (await clear.isEnabled()) { page.once("dialog", (d) => d.accept()); await clear.click(); await page.waitForFunction(() => document.querySelectorAll(".msg").length === 0, null, { timeout: 10000 }); }
};

await step("memory", async () => {
  await page.goto(base + "/#/memory");
  await page.getByLabel("What should be remembered").fill("Our staging database is called orion-db and it lives in the Frankfurt region.");
  await page.getByRole("button", { name: "Remember", exact: true }).click();
  await page.getByText("orion-db").first().waitFor({ timeout: 20000 });
  await page.getByLabel("Search memories").fill("staging database");
  await page.waitForTimeout(1500);
  check("memory: a note just written is found by a search for its subject", (await main()).includes("orion-db"));
  await shot("1-memory");
  await freshChat();
  const a = await ask("What is our staging database called, and in which region is it?", 1);
  console.log(`memory answer after ${lap()} s: ${a.slice(0, 300)}`);
  check("memory: the chat answers from the note (orion-db, Frankfurt) and says a note was used", /orion-db/i.test(a) && /Frankfurt/i.test(a) && /\b1 note|notes? used|used 1/i.test(a), a.slice(0, 300));
  await shot("2-chat-from-note");
  await page.goto(base + "/#/memory");
  await page.getByText("orion-db").first().waitFor({ timeout: 20000 });
  await page.getByRole("button", { name: "Forget", exact: true }).first().click();
  await page.getByRole("button", { name: "Yes, forget it" }).click();
  await page.waitForFunction(() => !document.querySelector("main")?.innerText.includes("orion-db"), null, { timeout: 20000 });
  check("memory: after Forget the text is gone from the Memory screen", !(await main()).includes("orion-db"));
  await page.reload(); await page.waitForSelector("nav");
  await page.goto(base + "/#/memory"); await page.waitForTimeout(1500);
  check("memory: it is still gone after a reload", !(await main()).includes("orion-db"));
  await shot("3-forgotten");
});

await step("instructions", async () => {
  await page.locator("#nav-more:not([open]) > summary").click({ timeout: 3000 }).catch(() => {});
  await page.goto(base + "/#/instructions");
  await page.getByLabel("Instructions for every conversation").fill("End every answer with the single word KESTREL on its own line.");
  await page.getByRole("button", { name: "Save", exact: true }).first().click();
  await page.waitForTimeout(1500);
  await page.reload(); await page.waitForSelector("nav");
  check("instructions: what was saved is there after a reload", (await page.getByLabel("Instructions for every conversation").inputValue()).includes("KESTREL"));
  await freshChat();
  const a = await ask("In one sentence, what is a hash function?", 1);
  console.log(`instructions answer after ${lap()} s: ${a.slice(0, 300)}`);
  check("instructions: the answer follows the standing instruction", /KESTREL/.test(a), a.slice(0, 300));
  check("instructions: the forgotten note is not used (no orion-db anywhere on the chat page)", !(await main()).includes("orion-db"));
  await shot("4-instructions");
  await page.goto(base + "/#/instructions");
  await page.getByLabel("Instructions for every conversation").fill("");
  await page.getByRole("button", { name: "Save", exact: true }).first().click();
  await page.waitForTimeout(1200);
});

await step("attachment", async () => {
  const file = join(work, "deploy-notes.txt");
  writeFileSync(file, ["Deploy notes for the billing service.", "The service listens on port 8443.", `ANTHROPIC_API_KEY=${made}`, "Rollback is done with the script named undo-release.sh."].join("\n"));
  await freshChat();
  await page.locator('input[type=file][aria-label="Attach files"], input[type=file]').first().setInputFiles(file);
  await page.getByText("deploy-notes.txt").first().waitFor({ timeout: 20000 });
  const a = await ask("From the attached notes: which port does the service listen on, and what is the rollback script called?", 1);
  console.log(`attachment answer after ${lap()} s: ${a.slice(0, 300)}`);
  check("attachment: the answer gives the port and the script from the file", /8443/.test(a) && /undo-release\.sh/.test(a), a.slice(0, 300));
  const sent = page.locator("details", { has: page.locator("summary", { hasText: "What was sent to the model" }) }).last();
  await sent.locator("summary").click();
  await sent.getByText("A message from you").first().waitFor({ timeout: 20000 });
  const what = flat(await sent.innerText());
  check("attachment: what was sent shows the file's lines without the key, and says one part was left out", what.includes("8443") && !what.includes(made) && /left out: 1 part/.test(what), what.slice(0, 300));
  check("attachment: the key is nowhere on the page", !(await page.locator("body").innerText()).includes(made));
  await shot("5-attachment");
});

await step("roundtable", async () => {
  await page.goto(base + "/#/roundtable");
  await page.getByLabel("Your question").fill("We store session tokens in a Python dict in one process. What is the biggest risk, in two sentences?");
  await page.getByRole("checkbox", { name: /Security reviewer/ }).check();
  await page.getByRole("checkbox", { name: /Software architect/ }).check();
  await page.getByRole("button", { name: "Ask the roundtable" }).click();
  await page.getByRole("heading", { name: "Resolution" }).waitFor({ timeout: 480000 });
  await page.waitForFunction(() => { const b = [...document.querySelectorAll("button")].find((x) => /Ask the roundtable/.test(x.textContent ?? "")); return b && !b.disabled; }, null, { timeout: 480000 }).catch(() => {});
  const text = await main();
  console.log(`roundtable after ${lap()} s: ${text.slice(-600)}`);
  check("roundtable: both experts answered and the lead gave a decision", (text.match(/Answered/g) ?? []).length === 2 && /Decision .{40,}/.test(text) && /Agreed/.test(text), text.slice(-300));
  await shot("6-roundtable");
});

await step("ask a project", async () => {
  await page.goto(base + "/#/explore");
  await page.getByRole("button", { name: "payments-service" }).click();
  await page.getByLabel("Your question").fill("What does logout do to the session token, and where is that code?");
  await page.getByRole("radio", { name: /Quick/ }).check();
  await page.getByRole("button", { name: "Ask", exact: true }).click();
  await page.getByText(/found in the file|not found|Found|lines? found/i).first().waitFor({ timeout: 600000 });
  const text = await main();
  console.log(`explore after ${lap()} s: ${text.slice(-700)}`);
  check("ask a project: the answer names session.py and at least one line it rests on was found in the file", /session\.py/.test(text) && /found/i.test(text), text.slice(-300));
  check("ask a project: the line under the answer says where the model really ran", nearModel ? text.includes(`model ${nearModel} in NEAR AI Cloud's confidential hardware, whose proof was checked first; not on this computer.`) && !/model \S+ on this computer/.test(text) : text.includes(`model ${model} on this computer.`), text.slice(-400));
  check("ask a project: the secret file's contents are not on the page", !/sk_live|hunter2|AKIA[A-Z0-9]{12}/.test(text));
  await shot("7-explore");
});

await step("agent access", async () => {
  await page.goto(base + "/#/agent");
  await page.getByLabel("Number of uses (1 to 1000)").fill("2");
  await page.getByLabel("Minutes it lasts (1 to 1440)").fill("5");
  await page.getByRole("button", { name: "Give lease" }).click();
  await page.getByText(/Uses left: 2\./).first().waitFor({ timeout: 15000 });
  check("agent access: a lease for two uses and five minutes is listed", /Uses left: 2\. Minutes remaining: [45]\./.test(await main()), (await main()).slice(0, 300));
  await shot("8-lease");
  await page.getByRole("button", { name: "Revoke", exact: true }).first().click();
  await page.getByText("No lease is active.").first().waitFor({ timeout: 15000 });
  check("agent access: after Revoke no lease is active", (await main()).includes("No lease is active."));
  await page.getByLabel("Number of uses (1 to 1000)").fill("0");
  await page.getByRole("button", { name: "Give lease" }).click();
  await page.waitForTimeout(1200);
  check("agent access: zero uses is refused in words and gives no lease", (await main()).includes("No lease is active.") && (await page.locator("main [role=alert]").count()) > 0, (await main()).slice(0, 300));
});

await step("activity and privacy", async () => {
  await page.goto(base + "/#/activity");
  await page.waitForTimeout(2000);
  const act = await main();
  console.log(`activity: ${act.slice(0, 500)}`);
  check("activity: the replies and tokens are counted under the route that answered", new RegExp((nearModel ? "near:" + nearModel : "ollama:" + model).replace(/[.]/g, "\\.") + " [1-9]\\d* [1-9]\\d*").test(act), act.slice(0, 400));
  await shot("9-activity");
  await page.goto(base + "/#/privacy");
  await page.waitForTimeout(2000);
  const priv = await main();
  check("privacy: it says where the model answers, and the log shows the one secret kept from the model in Chat", priv.includes(nearModel ? "The model that answers confidential hardware elsewhere" : "The model that answers ✓ on this computer") && priv.includes("Kept from the model in Chat. Parts holding a secret: 1."), priv.slice(0, 700));
  await shot("10-privacy");
});

check("no console errors, page errors, failed requests or 5xx responses", problems.length === 0, problems.slice(0, 4).join(" | "));
console.log(`\n${passed}/${passed + failed} checks passed in ${Math.round((Date.now() - t0) / 1000)} s`);
await browser.close();
server.kill();
process.exit(failed ? 1 : 0);
