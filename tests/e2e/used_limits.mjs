// A project kept on this computer while the model in use is elsewhere (NEAR AI Cloud), used as a person would: every way
// of working on that project must be refused in words and send nothing, and work again once the project allows
// confidential hardware. Run by hand with KAKU_TEST_NEAR_MODEL and KAKU_TEST_NEAR_KEY_FILE; it costs a fraction of a cent.
//
//   KAKU_TEST_NEAR_MODEL=NAME KAKU_TEST_NEAR_KEY_FILE=FILE node tests/e2e/used_limits.mjs
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
const work = join(repo, nearModel ? "target/used-limits-near" : "target/used-limits");
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

if (!nearModel) { console.log("needs KAKU_TEST_NEAR_MODEL: the limit only bites when the model is not on this computer"); process.exit(2); }
const replies = async () => {
  await page.goto(base + "/#/activity"); await page.waitForTimeout(1800);
  const m = new RegExp("near:" + nearModel.replace(/[.]/g, "\\.") + " (\\d+) ").exec(await main());
  return m ? Number(m[1]) : 0;
};
const refusal = "The project payments is kept more privately than the model in use now allows, so none of its material is sent to it. Choose a more private model in Settings, or change where the project's work may go under Projects.";
const say = async (name, text, n = 400) => console.log(`${name}: ${text.slice(0, n)}`);

await step("set up", async () => {
  await page.goto(base + "/#/projects");
  await page.getByLabel("Name").fill("payments");
  await page.getByLabel("Folder with its documents (optional)").fill(project);
  await page.getByRole("button", { name: "Save the project" }).click();
  await page.locator("#proj-list").getByText("payments").first().waitFor({ timeout: 15000 });
  await page.getByLabel("Where may its work go?").selectOption("here");
  await page.getByText("Saved where its work may go.").waitFor();
  check("setup: the project is kept on this computer", (await page.getByLabel("Where may its work go?").inputValue()) === "here");
});
const before = await replies();

await step("chat", async () => {
  await page.goto(base + "/#/projects");
  await page.getByRole("button", { name: "Chat in this project" }).click();
  await page.getByLabel("Your message").fill("Say the word MARLIN.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await page.waitForTimeout(6000);
  const text = await main();
  await say("chat", text.slice(text.indexOf("Chat")), 900);
  await shot("1-chat-refused");
  check("chat in the project: refused in words, no answer", !/MARLIN\b.*Answered in confidential/.test(text) && text.includes(refusal) && (await page.locator(".msg.assistant").count()) === 0, text.slice(0, 300));
});

await step("documents", async () => {
  await page.goto(base + "/#/projects");
  await page.getByRole("button", { name: "Ask its documents" }).click();
  await page.waitForSelector("#h-docs");
  await page.getByLabel(/question/i).first().fill("What does the README say about setup?");
  await page.getByRole("button", { name: /^Ask/ }).first().click();
  await page.waitForTimeout(8000);
  const text = await main();
  await say("documents", text, 1200);
  await shot("2-docs-refused");
  check("documents of the project: refused in words, no answer", (await page.locator("#h-answer").count()) === 0 && text.includes(refusal), text.slice(-400));
});

await step("ask a project", async () => {
  await page.goto(base + "/#/explore");
  await page.getByRole("button", { name: "payments-service" }).click();
  await page.getByLabel("Your question").fill("What does logout do?");
  await page.getByRole("radio", { name: /Quick/ }).check();
  await page.getByRole("button", { name: "Ask", exact: true }).click();
  await page.waitForTimeout(8000);
  const text = await main();
  await say("explore", text.slice(-700), 700);
  await shot("3-explore-refused");
  check("ask a project: refused in words, no answer", !/found at that line/.test(text) && text.includes(refusal), text.slice(-400));
});

await step("review", async () => {
  await page.goto(base + "/#/");
  await page.getByRole("button", { name: /Review a folder/ }).first().click().catch(() => {});
  await page.waitForTimeout(800);
  const text = await main();
  check("the review form says where the model runs: confidential hardware, not this computer", text.includes(`Model: ${nearModel} (runs in confidential hardware at cloud-api.near.ai, whose proof is checked before each request; not on this computer).`) && !text.includes("Your files stay on this computer."), text.slice(0, 600));
  await page.getByRole("button", { name: "payments-service" }).first().click();
  await page.getByLabel("Recipient name").fill("Acme audit");
  await page.getByRole("button", { name: "Start review" }).click();
  await page.waitForTimeout(6000);
  const said = await main();
  await say("review start", said.slice(-500), 500);
  check("a review of the kept project is refused in words and no review starts", said.includes(refusal) && !/#\/job\//.test(page.url()), said.slice(-400));
  await shot("4-review-form");
});

const after = await replies();
check("nothing was asked of the model elsewhere while the project was kept here", after === before, `before ${before}, after ${after}`);

await step("allow confidential hardware", async () => {
  await page.goto(base + "/#/projects");
  await page.getByLabel("Where may its work go?").selectOption({ index: 1 });
  await page.getByText("Saved where its work may go.").waitFor();
  console.log("now: " + (await page.getByLabel("Where may its work go?").inputValue()));
  await page.getByRole("button", { name: "Chat in this project" }).click();
  await page.getByLabel("Your message").fill("Say the word MARLIN.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await page.waitForFunction(() => document.querySelectorAll(".msg.assistant").length >= 1, null, { timeout: 120000 });
  await page.waitForTimeout(3000);
  const a = flat(await page.locator(".msg.assistant").first().innerText());
  check("once the project allows confidential hardware, the chat answers there", /MARLIN/i.test(a) && /confidential hardware/.test(a), a.slice(0, 300));
});

// the four refusals are answered 409, and the browser logs each
const other = problems.filter((x) => !/status of 409/.test(x) && !(/status of 400/.test(x) && /\/api\/jobs/.test(x)));
check("no console errors beyond the four refusals", other.length === 0 && problems.length === 4, problems.join(" | "));
console.log(`\n${passed}/${passed + failed} checks passed in ${Math.round((Date.now() - t0) / 1000)} s`);
await browser.close();
server.kill();
process.exit(failed ? 1 : 0);
