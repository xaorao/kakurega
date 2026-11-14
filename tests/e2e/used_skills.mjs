// Skills and outside tools, used as a person uses them with a real local model (run by hand; minutes, and the model's
// words differ run to run): a skill from the shelf is added, read, approved and then shapes an answer; a method of one's
// own waits until approved and waits again after an edit; a real Model Context Protocol server (the reference
// "everything" server, started through a small wrapper) is added, its real tools are listed, one is allowed, the choice
// survives a reload, and the program is removed. The server is fetched by npx, so this run needs the network.
//
//   node tests/e2e/used_skills.mjs [MODEL]       default qwen2.5-coder:7b; writes target/used-skills/*.png
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
const work = join(repo, nearModel ? "target/used-skills-near" : "target/used-skills");
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
page.on("console", (m) => { if (m.type() === "error" && !(/status of 422/.test(m.text()) && /\/api\/mcp\/discover/.test(m.location()?.url ?? ""))) problems.push("console: " + m.text().slice(0, 200) + " " + (m.location()?.url ?? "")); });
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


import { chmodSync } from "node:fs";
const wrapper = join(work, "everything-server.sh");
// first as a person would write it; the program is started with a fixed search path, so this one cannot find node
writeFileSync(wrapper, "#!/bin/sh\nexec /opt/homebrew/bin/npx -y @modelcontextprotocol/server-everything@2.3.0\n");
chmodSync(wrapper, 0o755);
const toSkills = async () => { await page.goto(base + "/#/skills"); await page.getByRole("heading", { name: "Methods" }).waitFor({ timeout: 20000 }); };

await step("shelf skill", async () => {
  await toSkills();
  const row = page.locator('#bundled-methods li[data-bundled="release-notes"]');
  await row.getByRole("button", { name: "Add release-notes" }).click();
  await page.getByText("Added release-notes. Read it above and approve it to use it.").waitFor({ timeout: 15000 });
  await freshChat();
  const before = await ask("Write release notes for version 2.4: logout now ends the session on the server; added CSV export of tables; fixed a crash when a folder is empty.", 1);
  check("shelf: before approval the answer does not say a method was used", !/Methods? used/i.test(before), before.slice(-300));
  await toSkills();
  await page.getByRole("button", { name: "Show text" }).first().click();
  const text = await main();
  console.log("skill text: " + text.slice(text.indexOf("release-notes"), text.indexOf("release-notes") + 500));
  await shot("1-skill-text");
  await page.getByRole("button", { name: "Approve this text" }).first().click();
  await page.getByText("Approved: in use").waitFor({ timeout: 15000 });
  await freshChat();
  const after = await ask("Write release notes for version 2.4: logout now ends the session on the server; added CSV export of tables; fixed a crash when a folder is empty.", 1);
  console.log(`release notes after ${lap()} s: ${after.slice(0, 700)}`);
  check("shelf: after approval the answer says the method was used and covers the three changes", /Methods? used/i.test(after) && /release-notes/.test(after) && /log(ging)? ?out/i.test(after) && /CSV/i.test(after) && /crash|empty folder/i.test(after), after.slice(0, 400));
  await shot("2-release-notes");
});

await step("own method", async () => {
  await toSkills();
  await page.getByRole("button", { name: "Write a method of your own" }).click();
  await page.locator("#met-new-name").fill("signoff");
  await page.locator("#met-new-desc").fill("How to end an answer about birds");
  await page.locator("#met-new-trig").fill("heron, kingfisher");
  await page.locator("#met-new-body").fill("End your answer with the single word OSPREY on its own line.");
  await page.getByRole("button", { name: /^Save \(it will wait/ }).click();
  await page.getByText("Saved. Read it, then approve it to use it.").waitFor({ timeout: 15000 });
  await freshChat();
  const a1 = await ask("In one sentence, what does a heron eat?", 1);
  check("own method: unapproved, it is not followed", !/OSPREY/.test(a1), a1.slice(0, 300));
  await toSkills();
  // the shelf skill is already approved, so the one method still waiting is this one
  await page.getByRole("button", { name: "Show text" }).last().click();
  await page.getByRole("button", { name: "Approve this text" }).first().waitFor({ timeout: 15000 });
  await page.getByRole("button", { name: "Approve this text" }).first().click();
  await page.waitForTimeout(1500);
  await freshChat();
  const a2 = await ask("In one sentence, what does a heron eat?", 1);
  console.log(`own method answer: ${a2.slice(0, 300)}`);
  check("own method: approved, a question that matches it is answered its way", /OSPREY/.test(a2) && /signoff/.test(a2), a2.slice(0, 300));
  const a3 = await (async () => { await freshChat(); return ask("In one sentence, what is a hash function?", 1); })();
  check("own method: a question it does not match is left alone", !/OSPREY/.test(a3), a3.slice(0, 300));
});

await step("outside tool", async () => {
  await toSkills();
  await page.getByRole("button", { name: "Add an outside program" }).click();
  await page.getByLabel("A short name for it").fill("everything");
  await page.getByLabel("Full path of the program").fill(wrapper);
  await page.getByRole("button", { name: "Add program" }).click();
  await page.getByText("Added. Check its tools next.").waitFor({ timeout: 20000 });
  await page.getByRole("button", { name: "Check its tools" }).click();
  await page.getByText(/the server did not start/).waitFor({ timeout: 120000 });
  const why = await main();
  console.log("first try: " + why.slice(why.indexOf("the server did not start"), why.indexOf("the server did not start") + 420));
  check("outside tool: a program that cannot start says what it printed and how it was started", /the program said: .*node/.test(why) && why.includes("PATH=/usr/bin:/bin") && why.includes("has to set it itself"), why.slice(why.indexOf("the server did not start"), why.indexOf("the server did not start") + 420));
  // do what the message says: the wrapper sets its own search path; a changed program has to be added again
  await page.getByRole("button", { name: "Remove" }).last().click();
  await page.getByText("No outside programs yet.").waitFor({ timeout: 15000 });
  writeFileSync(wrapper, "#!/bin/sh\nexport PATH=/opt/homebrew/bin:/usr/bin:/bin\nexport HOME=" + JSON.stringify(join(work, "home")) + "\nexec npx -y @modelcontextprotocol/server-everything@2.3.0\n");
  await page.getByRole("button", { name: "Add an outside program" }).click();
  await page.getByLabel("A short name for it").fill("everything");
  await page.getByLabel("Full path of the program").fill(wrapper);
  await page.getByRole("button", { name: "Add program" }).click();
  await page.getByText("Added. Check its tools next.").waitFor({ timeout: 20000 });
  await page.getByRole("button", { name: "Check its tools" }).click();
  await page.getByText(/Checked: \d+ tools?\./).waitFor({ timeout: 240000 });
  const text = await main();
  const n = Number(/Checked: (\d+) tools?\./.exec(text)?.[1] ?? 0);
  console.log(`outside tools after ${lap()} s: ${n} tools; ${text.slice(text.indexOf("Checked:"), text.indexOf("Checked:") + 700)}`);
  check("outside tool: the real server's tools are listed, echo among them", n >= 5 && (await page.locator("#mcp-everything-echo").count()) === 1, `n=${n}`);
  check("outside tool: every tool starts switched off", await page.locator('select[id^="mcp-everything-"]').evaluateAll((els) => els.length > 0 && els.every((e) => e.value === "off")));
  await shot("3-tools");
  await page.locator("#mcp-everything-echo").selectOption("ask");
  await page.waitForTimeout(1200);
  await page.reload(); await page.waitForSelector("nav");
  await toSkills();
  check("outside tool: the choice for echo is kept after a reload, the rest stay off", (await page.locator("#mcp-everything-echo").inputValue()) === "ask" && (await page.locator('select[id^="mcp-everything-"]').evaluateAll((els) => els.filter((e) => e.value !== "off").length)) === 1);
  await page.getByRole("button", { name: "Remove" }).last().click();
  await page.getByText("No outside programs yet.").waitFor({ timeout: 15000 });
  check("outside tool: removed, nothing of it is left", (await page.locator('[id^="mcp-everything-"]').count()) === 0);
});

check("no console errors, page errors, failed requests or 5xx responses", problems.length === 0, problems.slice(0, 4).join(" | "));
console.log(`\n${passed}/${passed + failed} checks passed in ${Math.round((Date.now() - t0) / 1000)} s`);
await browser.close();
server.kill();
process.exit(failed ? 1 : 0);
