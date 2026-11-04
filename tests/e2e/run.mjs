import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/e2e-tmp");
const shots = join(repo, "target/e2e-shots");
const kaku = join(repo, "target/debug/kaku");
rmSync(work, { recursive: true, force: true });
mkdirSync(shots, { recursive: true });
const root = join(work, "root");
const project = join(root, "payments-service");
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
mkdirSync(join(root, "contracts"), { recursive: true });
writeFileSync(join(root, "contracts/agreement.md"), "The term of this agreement is twelve (12) months from the Effective Date.\nEither party may terminate with sixty (60) days notice.\n");
writeFileSync(join(root, "contracts/.env"), "API_TOKEN=" + "sk_" + "live_abcdefghijklmnop0123456789\n");
const data = join(work, "data");

const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const results = [];
const axeSource = readFileSync(join(here, "node_modules/axe-core/axe.min.js"), "utf8");
const axeCheck = async (pg, label) => {
  await pg.evaluate(axeSource);
  const res = await pg.evaluate(async () => await window.axe.run(document, { runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "best-practice"] } }));
  const bad = res.violations.filter((v) => ["serious", "critical", "moderate"].includes(v.impact));
  check(`accessibility (axe, WCAG 2.1 AA): ${label}`, bad.length === 0, bad.map((v) => `${v.id}[${v.impact}] x${v.nodes.length}: ${v.nodes[0]?.target?.join(" ")}`).join(" | ").slice(0, 500));
};
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  " + detail : ""}`);
};

const server = spawn(kaku, ["serve", "--data-dir", data, "--root", root, "--scripted", join(here, "script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let out = "";
server.stdout.on("data", (d) => (out += d));
server.stderr.on("data", (d) => (out += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(out) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(out)?.[0];
assert.ok(link, "kaku serve did not print a pairing link:\n" + out);
const base = new URL(link).origin;

const problems = [];
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } });
const page = await ctx.newPage();
const requestOrigins = new Set();
page.on("request", (r) => { try { requestOrigins.add(new URL(r.url()).origin); } catch { /* data: urls */ } });
page.on("console", (m) => {
  if (m.type() !== "error") return;
  // The browser logs every non-2xx fetch as a console error. The model-proof refusal is a deliberate 422 that the page
  // shows as a message (checked by its own steps), so only that exact response is excused.
  if (m.location().url.endsWith("/api/route/attest") && /status of 422/.test(m.text())) return;
  problems.push("console: " + m.text());
});
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", async (r) => {
  // Classified as a harness artifact (QA guide 12.4): Chromium reports ERR_ABORTED for a streamed (chunked) response whose
  // body was fully read, in some runs and not others. Only that case is excused: a 200 response was received and the page
  // showed the complete answer (checked by the chat steps); a request without a response still counts as a failure.
  const response = await r.response().catch(() => null);
  if (r.url().endsWith("/api/chat/stream") && r.failure()?.errorText === "net::ERR_ABORTED" && response?.status() === 200) return;
  // Also classified as a harness artifact: an idempotent GET that the browser cancelled because the page was reloaded or
  // navigated while it was in flight (the screens poll /api/fs, /api/readiness and /api/jobs). A POST or any other failure
  // still counts.
  if (r.method() === "GET" && r.failure()?.errorText === "net::ERR_ABORTED" && /\/(api\/(fs|readiness|jobs|sessions|memories|inbox|events|schedules)|favicon\.svg)(\?|$)/.test(new URL(r.url()).pathname + new URL(r.url()).search)) return;
  problems.push("requestfailed: " + r.url() + " " + r.failure()?.errorText);
});
page.on("response", (r) => r.status() >= 500 && problems.push("http " + r.status() + " " + r.url()));

try {
  await page.goto(link);
  await page.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 15000 });
  check("pairing link signs the browser in and clears the code from the address bar", !page.url().includes("pair="));
  await page.getByRole("heading", { name: "Ready to review" }).waitFor({ timeout: 20000 });
  check("a correctly set up machine says Ready to review and lists every check as ready", (await page.getByRole("region", { name: "Setup check" }).innerText()).split("Needs attention").length === 1);
  await page.screenshot({ path: join(shots, "01-new-review.png") });
  await axeCheck(page, "new review");

  const other = await browser.newContext();
  const reuse = await other.newPage();
  await reuse.goto(link);
  await reuse.getByRole("heading", { name: "Pair this browser" }).waitFor({ timeout: 15000 });
  await reuse.getByRole("alert").waitFor({ timeout: 15000 });
  check("a used pairing link does not work a second time", /wrong, used or expired/.test(await reuse.getByRole("alert").innerText()));
  await other.close();

  await page.getByRole("button", { name: "payments-service" }).click();
  await page.waitForFunction(() => document.querySelector(".path")?.textContent?.endsWith("payments-service"));
  await page.getByLabel("Recipient name").fill("acme-audit");
  await page.getByRole("button", { name: "Start review" }).click();
  await page.getByRole("tab", { name: "Release" }).waitFor({ timeout: 120000 });
  await page.getByRole("heading", { name: "The exact release" }).waitFor({ timeout: 120000 });
  const disc = page.getByRole("region", { name: "Cumulative disclosure" });
  await disc.waitFor();
  check("the release tab shows how much of the project this recipient has now seen", /\d+ of this project's \d+ files would have been shown/.test(await disc.innerText()));
  await disc.locator("summary").click();
  check("and says what was not checked", (await disc.innerText()).includes("combined excerpts"));
  await axeCheck(page, "release tab with the disclosure card");
  check("review runs in the sandbox and opens on the release", true);
  await page.screenshot({ path: join(shots, "02-release.png"), fullPage: true });
  await axeCheck(page, "release");
  const digestText = await page.locator("p", { hasText: "Digest" }).first().innerText();
  const digest = /[0-9a-f]{64}/.exec(digestText)?.[0];
  check("release shows a full digest", !!digest);
  const releaseText = await page.locator('[role="tabpanel"]').innerText();
  check("release lists the fixed files and the held-back file count", releaseText.includes("finding-1.txt") && releaseText.includes("run-check.py") && /1 withheld file/.test(releaseText));
  check("release states what it does not establish", /does not establish/i.test(releaseText) && /private source repository/.test(releaseText));
  check("no secret value appears in the release", !/hunter2|sk_live/.test(releaseText));

  await page.getByRole("tab", { name: "Source" }).click();
  await page.getByRole("button", { name: /\.env/ }).click();
  await page.getByText("This file is labelled restricted").waitFor();
  const srcText = await page.locator(".viewer").innerText();
  check("withheld file is shown as withheld, never its content", /Withheld/.test(srcText) && !/hunter2|sk_live/.test(srcText));
  await page.getByRole("button", { name: /payments\/auth\/session\.py/ }).click();
  await page.getByText("def logout").waitFor();
  await page.screenshot({ path: join(shots, "03-source.png") });
  await axeCheck(page, "source");

  await page.getByRole("tab", { name: "Findings" }).click();
  await page.getByRole("tab", { name: "Findings" }).press("ArrowRight");
  check("the arrow keys move between the review tabs and keep focus on the selected one", (await page.getByRole("tab", { name: "Release" }).getAttribute("aria-selected")) === "true" && (await page.evaluate(() => document.activeElement?.textContent)) === "Release");
  await page.getByRole("tab", { name: "Release" }).press("Home");
  check("Home goes to the first tab", (await page.getByRole("tab", { name: "Progress" }).getAttribute("aria-selected")) === "true");
  await page.getByRole("tab", { name: "Findings" }).click();
  await page.getByText("Confirmed by the kernel").waitFor();
  check("finding is confirmed by the kernel", true);

  await page.getByRole("tab", { name: "Protection" }).click();
  await page.getByText("possible prompt injection").first().waitFor();
  await page.getByText("refused: UnknownHandle").first().waitFor({ timeout: 15000 });
  const prot = await page.locator('[role="tabpanel"]').innerText();
  check("protection lists withheld files, injection suspects and refused requests", prot.includes(".env") && prot.includes("README.md") && /refused: UnknownHandle/.test(prot), prot.includes("refused") ? "" : "no refused rows");
  await page.screenshot({ path: join(shots, "04-protection.png"), fullPage: true });
  await axeCheck(page, "protection");

  await page.getByRole("tab", { name: "Release" }).click();
  const noCapsuleYet = await page.getByRole("button", { name: "Sign and deliver the capsule" }).count();
  check("cannot deliver before approval", noCapsuleYet === 0);
  await page.getByRole("button", { name: "Request approval" }).click();
  await page.getByText("Waiting for you.").waitFor();
  const cmd = await page.locator("pre.cmd").innerText();
  check("page tells the user to approve in the terminal", /kaku approve appr-\d+/.test(cmd));
  await page.screenshot({ path: join(shots, "05-waiting.png"), fullPage: true });
  await axeCheck(page, "waiting for approval");

  const approve = spawnSync(kaku, ["approve", "--data-dir", data], { input: digest.slice(0, 12) + "\n", encoding: "utf8" });
  check("terminal shows the exact files and accepts the digest prefix", approve.status === 0 && /RELEASE FOR APPROVAL/.test(approve.stdout) && /Approved\./.test(approve.stdout), approve.stderr.slice(0, 200));
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).waitFor({ timeout: 15000 });
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).click();
  await page.getByText("Delivered.").waitFor();

  // history search, Cmd+K, theme choice
  await page.keyboard.press("Meta+k");
  check("Cmd+K focuses the review search", await page.evaluate(() => document.activeElement?.id === "search"));
  await page.keyboard.type("acme");
  check("searching by recipient keeps the matching review", (await page.locator(".list li a").count()) === 1);
  await page.fill("#search", "nothing-like-this");
  check("a search with no match says so", await page.getByText("No review matches.").isVisible());
  await page.keyboard.press("Escape");
  check("Escape clears the search and shows the list again", (await page.inputValue("#search")) === "" && (await page.locator(".list li a").count()) === 1);
  await page.getByLabel("Theme").selectOption("dark");
  check("choosing Dark sets the theme", (await page.evaluate(() => document.documentElement.getAttribute("data-theme"))) === "dark");
  await page.reload();
  await page.getByLabel("Theme").waitFor();
  check("the theme choice survives a reload", (await page.evaluate(() => document.documentElement.getAttribute("data-theme"))) === "dark");
  await new Promise((r) => setTimeout(r, 400));
  await page.screenshot({ path: join(shots, "09-dark-review.png") });
  await axeCheck(page, "review release tab, dark theme");
  await page.getByLabel("Language").selectOption("hi");
  await page.getByRole("heading", { name: "सटीक रिलीज़" }).waitFor();
  check("choosing Hindi renders the release screen in Hindi", (await page.getByRole("tab", { name: "रिलीज़" }).count()) === 1 && (await page.getByRole("button", { name: "कैप्सूल पर हस्ताक्षर करके भेजें" }).count()) <= 1);
  check("the page language attribute follows the choice", (await page.evaluate(() => document.documentElement.lang)) === "hi");
  await new Promise((r) => setTimeout(r, 400));
  await page.screenshot({ path: join(shots, "10-hindi-release.png") });
  await axeCheck(page, "review release tab, Hindi, dark theme");
  const leftover = await page.evaluate(() => [...document.querySelectorAll("h1,h2,h3,button,th,label,summary")].map((e) => e.textContent.trim()).filter((x) => /^[A-Za-z][A-Za-z ,.']{3,}$/.test(x) && !["SHA-256"].includes(x)));
  check("no English interface headings or buttons remain on the Hindi release screen", leftover.length === 0, leftover.join(" | "));
  await page.getByLabel("भाषा").selectOption("en");
  await page.getByRole("heading", { name: "The exact release" }).waitFor();
  check("switching back restores English", (await page.evaluate(() => document.documentElement.lang)) === "en");
  await page.getByLabel("Theme").selectOption("system");
  check("Match system removes the override", (await page.evaluate(() => document.documentElement.getAttribute("data-theme"))) === null);

  // memory: add, refuse a secret, duplicate, search, correct with history, forget for good
  await page.getByRole("link", { name: "Memory" }).click();
  await page.getByRole("heading", { name: "Memory", exact: true }).waitFor();
  check("the memory screen starts empty", await page.getByText("Nothing remembered yet.").isVisible());
  await page.getByLabel("What should be remembered").fill("Acme prefers invoices in euros");
  await page.getByRole("button", { name: "Remember" }).click();
  await page.getByText("Remembered.").waitFor();
  check("a remembered item is listed with the reason it is kept", (await page.getByText("Why: you asked to remember it: you added it in the Memory screen").count()) === 1);
  await page.getByLabel("What should be remembered").fill("the password is hunter2hunter2");
  await page.getByRole("button", { name: "Remember" }).click();
  await page.getByRole("alert").filter({ hasText: "secrets are never stored" }).waitFor();
  check("a secret is refused with a clear reason and is not listed", (await page.locator(".mem", { hasText: "hunter2" }).count()) === 0);
  await page.getByLabel("What should be remembered").fill("  acme PREFERS invoices in euros ");
  await page.getByRole("button", { name: "Remember" }).click();
  await page.getByText("Already remembered.").waitFor();
  check("the same memory in another casing is not added twice", (await page.locator(".mem").count()) === 1);
  await page.getByLabel("Search memories").fill("zzzz");
  await page.getByText("No memory matches.").waitFor();
  await page.getByLabel("Search memories").fill("euros");
  await page.locator(".mem").first().waitFor();
  check("searching finds the memory", (await page.locator(".mem").count()) === 1);
  await page.getByLabel("Search memories").fill("");
  await axeCheck(page, "memory screen");
  await page.getByRole("button", { name: "Correct" }).click();
  await page.locator("textarea[id^=edit-]").fill("Acme prefers invoices in pounds");
  await page.getByRole("button", { name: "Save correction" }).click();
  await page.getByText("Acme prefers invoices in pounds").waitFor();
  check("a correction replaces the memory in the list", (await page.getByText("invoices in euros").count()) === 0);
  await page.getByRole("button", { name: "History" }).click();
  await page.getByText("replaced").waitFor();
  check("history keeps the old wording next to the current one", (await page.locator(".mem li").count()) === 2);
  await page.screenshot({ path: join(shots, "11-memory.png") });
  await page.getByRole("button", { name: "Forget", exact: true }).click();
  check("forgetting asks for confirmation first and says it is permanent", await page.getByText("Forget this for good?").isVisible());
  await page.getByRole("button", { name: "Yes, forget it" }).click();
  await page.getByText("Nothing remembered yet.").waitFor();
  await page.getByLabel("What should be remembered").fill("Acme prefers invoices in pounds");
  await page.getByRole("button", { name: "Remember" }).click();
  await page.getByRole("alert").filter({ hasText: "you asked to forget this" }).waitFor();
  check("after forgetting, the same text cannot be saved again", (await page.locator(".mem").count()) === 0);
  const dbBytes = readFileSync(join(data, "memory.db"));
  check("the memory file on disk is encrypted (no readable memory text)", !dbBytes.includes("invoices") && !dbBytes.includes("Acme"));
  for (let k = problems.length - 1; k >= 0; k--) if (/status of 422/.test(problems[k])) problems.splice(k, 1); // the two refusals above are the expected answers
  await page.locator('.list a[href^="#/job/"]').first().click();
  await page.getByRole("heading", { name: "payments-service" }).waitFor();
  // suggestions from the review: shown, flagged untrusted, nothing saved until accepted
  await page.getByRole("tab", { name: "Findings" }).click();
  const sugg = page.getByRole("region", { name: "Suggested memories" });
  await sugg.waitFor();
  check("the review's two suggestions are shown, flagged untrusted", (await sugg.locator("li").count()) === 2 && (await sugg.getByText("from untrusted content").count()) === 2);
  await axeCheck(page, "findings tab with suggested memories");
  // proposed fixes: kept for the owner, one accepted by the reproducer and one refused, applied only after a second confirmation
  const fixesCard = page.getByRole("region", { name: "Proposed fixes" });
  await fixesCard.waitFor();
  const fixText = await fixesCard.innerText();
  check("two proposed fixes are shown with the limit stated, one accepted by the reproducer and one not accepted", (await fixesCard.locator("li").count()) === 2 && fixText.includes("project's own tests were not run") && fixText.includes("The reproducer stops reproducing") && fixText.includes("Not accepted"));
  await fixesCard.locator("details").first().locator("summary").click();
  const diffBox = fixesCard.locator("pre.diff").first();
  check("the change is shown as a readable diff with signs, line numbers and the removed and added lines marked", (await diffBox.locator(".dl.add").count()) >= 1 && (await diffBox.locator(".dl.del").count()) >= 1 && (await diffBox.locator(".dl.add .sign").first().innerText()) === "+" && (await diffBox.locator(".dl.del .sign").first().innerText()) === "-" && (await diffBox.innerText()).includes("ACTIVE_TOKENS.pop(token, None)") && /\d/.test(await diffBox.locator(".dl.del .ln").first().innerText()));
  await axeCheck(page, "fix diff open");
  check("only the accepted fix offers Apply", (await fixesCard.getByRole("button", { name: /^Apply to/ }).count()) === 1);
  await axeCheck(page, "findings tab with proposed fixes");
  await page.screenshot({ path: join(shots, "15-fixes.png"), fullPage: true });
  const sessionPath = join(project, "payments/auth/session.py");
  const beforeFix = readFileSync(sessionPath, "utf8");
  await fixesCard.getByRole("button", { name: /^Apply to/ }).click();
  await fixesCard.getByRole("alertdialog").waitFor();
  await fixesCard.getByRole("button", { name: "Cancel" }).click();
  check("cancelling the second confirmation changes nothing", readFileSync(sessionPath, "utf8") === beforeFix);
  await fixesCard.getByRole("button", { name: /^Apply to/ }).click();
  await fixesCard.getByRole("button", { name: "Yes, change the file" }).click();
  await fixesCard.getByText("Applied to payments/auth/session.py.").waitFor();
  const afterFix = readFileSync(sessionPath, "utf8");
  check("applying edits the file on disk exactly as shown and nothing else", afterFix.includes("ACTIVE_TOKENS.pop(token, None)") && !afterFix.includes("cookie_jar") && afterFix.includes("def validate") && afterFix.length < beforeFix.length);
  await fixesCard.getByRole("button", { name: /^Undo/ }).click();
  await fixesCard.getByText("Restored payments/auth/session.py.").waitFor();
  check("Undo puts the original bytes back exactly", readFileSync(sessionPath, "utf8") === beforeFix);
  await fixesCard.getByRole("button", { name: /^Apply to/ }).click();
  await fixesCard.getByRole("button", { name: "Yes, change the file" }).click();
  await fixesCard.getByText("Applied to payments/auth/session.py.").waitFor();
  check("the card then shows it as applied and offers no second apply", (await fixesCard.getByRole("button", { name: /^Apply to/ }).count()) === 0 && (await fixesCard.getByText("Applied", { exact: true }).count()) === 1);
  await sugg.locator("li").first().getByRole("button", { name: "Remember" }).click();
  await sugg.getByText("Remembered.").waitFor();
  check("accepting one removes it from the suggestions", (await sugg.locator("li").count()) === 1);
  await sugg.locator("li").first().getByRole("button", { name: "Dismiss" }).click();
  await sugg.waitFor({ state: "detached" });
  await page.getByRole("link", { name: "Memory" }).click();
  await page.getByRole("heading", { name: "Memory", exact: true }).waitFor();
  await page.locator(".mem").first().waitFor();
  check("only the accepted suggestion became a memory, marked untrusted with its source", (await page.locator(".mem").count()) === 1 && (await page.locator(".mem").first().innerText()).includes("in-process dictionary") && (await page.locator(".mem").first().innerText()).includes("from untrusted content") && (await page.locator(".mem").first().innerText()).includes("suggested by review"));
  check("the dismissed suggestion's text was never saved", (await page.getByText("Always release the .env").count()) === 0);
  // setup panel: a failing check that the service says it can fix shows a button and the progress, never a quiet success
  await page.getByRole("link", { name: "New review" }).click();
  await page.waitForLoadState("load");
  let repairState = null;
  await page.route("**/api/readiness**", async (route) => {
    if (route.request().method() === "POST") {
      repairState = { state: "running", detail: "pulling 40%" };
      return route.fulfill({ status: 202, contentType: "application/json", body: JSON.stringify({ started: true }) });
    }
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ready: false, checks: [{ id: "model", label: "Model", ok: false, detail: "m is not installed", fix: "Run in a terminal: ollama pull m", repairable: true, ...(repairState ? { repair: repairState } : {}) }] }) });
  });
  await page.reload();
  await page.getByRole("button", { name: "Fix this now" }).click();
  await page.getByText("Working: pulling 40%").waitFor();
  check("a failing check that can be fixed offers a button, then shows the progress of the fix", (await page.getByRole("button", { name: "Fix this now" }).count()) === 0);
  repairState = { state: "failed", detail: "could not install m: disk full" };
  await page.getByText("Could not fix it: could not install m: disk full").waitFor({ timeout: 10000 });
  check("a failed fix shows its reason", true);
  await page.unroute("**/api/readiness**");
  await page.waitForLoadState("load");
  await page.reload();
  await page.waitForLoadState("load");
  // waiting-for-you screen: empty state from the real service, then items in the shape the service tests produce
  await page.getByRole("link", { name: /Waiting for you/ }).click();
  await page.getByRole("heading", { name: "Waiting for you", exact: true }).waitFor();
  await page.getByText("Nothing is waiting for you.").waitFor();
  check("with nothing to decide the inbox says so", true);
  await axeCheck(page, "waiting for you screen (empty)");
  await page.route("**/api/inbox", (route) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ count: 2, items: [{ kind: "approval_waiting", job: "job-9", recipient: "Acme", since: 5, what: "An approval is waiting for your decision." }, { kind: "failed", job: "job-8", recipient: "Beta", since: 4, what: "The review failed. Open it to see why." }] }) }));
  await page.reload();
  await page.getByText("Approval waiting").waitFor();
  const itemsText = await page.locator("main").innerText();
  check("each waiting item names what it is, who it is for and links to the review", itemsText.includes("Approval waiting") && itemsText.includes("Acme") && itemsText.includes("Failed") && (await page.getByRole("link", { name: "Open" }).first().getAttribute("href")) === "#/job/job-9");
  await axeCheck(page, "waiting for you screen (items)");
  await page.unroute("**/api/inbox");
  await page.waitForLoadState("load");
  await page.reload();
  await page.waitForLoadState("load");
  // repeating reviews: add one for a chosen folder and recipient, see it listed, remove it
  await page.getByRole("link", { name: "New review" }).click();
  await page.getByRole("button", { name: "payments-service" }).click();
  await page.locator("#recipient").fill("Nightly auditor");
  await page.getByText("Repeat this review automatically").click();
  await page.getByRole("button", { name: "Add this schedule" }).click();
  await page.getByText("every 24 h").waitFor();
  check("a repeating review is listed with its folder, recipient and cadence, and the card says nothing is approved or sent without the owner", (await page.locator("#schedules").innerText()).includes("Nightly auditor") && (await page.locator("#schedules").innerText()).includes("nothing is approved or sent without you"));
  await axeCheck(page, "repeat review card");
  await page.locator("#schedules").getByRole("button", { name: "Remove" }).click();
  await page.getByText("No repeating reviews.").waitFor();
  check("removing the schedule empties the list", true);
  // an interrupted review can be run again with the same folder and recipient
  const dead = { id: "job-77", repo: "/some/folder", recipient: "Acme", created: 1, state: "failed", note: "the service stopped while this review was running; start it again", events: [{ at: 1, stage: "interrupted", detail: "the service stopped" }], summary: null, approval: null, delivered: null };
  let startedAgain = null;
  await page.route("**/api/jobs**", async (route) => {
    const req = route.request();
    if (req.method() === "POST") {
      startedAgain = JSON.parse(req.postData() ?? "{}");
      return route.fulfill({ status: 202, contentType: "application/json", body: JSON.stringify({ id: "job-78" }) });
    }
    if (req.url().endsWith("/api/jobs")) return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ jobs: [dead] }) });
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(dead) });
  });
  await page.goto(base + "/#/job/job-77");
  await page.reload();
  await page.getByRole("button", { name: "Run this review again" }).click();
  await page.waitForURL(/#\/job\/job-78/);
  check("an interrupted review offers to run again and starts the same folder for the same recipient", startedAgain?.repo === "/some/folder" && startedAgain?.recipient === "Acme");
  await page.goto(base + "/#/");
  await page.getByRole("heading", { name: "Review a project" }).waitFor();
  await page.unroute("**/api/jobs**");
  await page.reload();
  // command palette: keyboard-driven, focus returns where it was, and it can change settings
  await page.getByRole("button", { name: "Commands" }).focus();
  await page.keyboard.press("Control+Shift+P");
  const palette = page.getByRole("dialog", { name: "Commands" });
  await palette.waitFor();
  check("the command palette opens with the keyboard and puts focus in its search box", await palette.getByRole("combobox").evaluate((el) => el === document.activeElement));
  await page.keyboard.type("memory");
  check("typing narrows the commands to matching ones", (await palette.getByRole("option").count()) === 1);
  await axeCheck(page, "command palette");
  await page.keyboard.press("Enter");
  await page.getByRole("heading", { name: "Memory", exact: true }).waitFor();
  check("choosing a command goes there, closes the palette and returns focus", (await palette.count()) === 0 && (await page.evaluate(() => document.activeElement?.textContent)) === "Commands");
  await page.keyboard.press("Control+Shift+P");
  await page.keyboard.type("dark");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  check("a setting can be changed from the palette", (await page.evaluate(() => document.documentElement.getAttribute("data-theme"))) === "dark");
  await page.keyboard.press("Control+Shift+P");
  await page.keyboard.type("zzzznothing");
  check("a search with no match says so", (await palette.getByText("Nothing matches.").count()) === 1);
  await page.keyboard.press("Escape");
  check("Escape closes the palette", (await palette.count()) === 0);
  // chat: a private assistant turn that can use a saved note, and nothing is saved
  await page.getByRole("link", { name: "Chat" }).click();
  await page.getByRole("heading", { name: "Chat", exact: true }).waitFor();
  check("the chat says it is private and that conversations are not saved", (await page.getByText("Conversations are not saved").count()) === 1);
  // model proof panel: the response shapes are the ones the service tests and the live run produce
  await page.route("**/api/route/attest", async (route) => {
    const model = JSON.parse(route.request().postData() ?? "{}").model;
    if (model === "good/model") {
      await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ verified: true, model, means: ["Intel TDX hardware vouched for the software that is running."], does_not_mean: ["It does not prove that a particular answer came from this software."], deployments: [{ signer: "0xsignerAAA", tcb: "UpToDate", advisories: [], gpu_verified: true, compose_hash: "abc123", os_image_hash: null, nonce: "nonce777" }] }) });
    } else {
      await route.fulfill({ status: 422, contentType: "application/json", body: JSON.stringify({ verified: false, model, error: { code: "NOT_VERIFIED", message: "attestation verified but the route policy refuses it: TcbNotAccepted" } }) });
    }
  });
  await page.getByText("Check a confidential model's hardware proof").click();
  await page.getByLabel("Model name").fill("good/model");
  await page.getByRole("button", { name: "Check now" }).click();
  const proofRegion = page.getByRole("region", { name: "Hardware proof result" });
  await proofRegion.waitFor();
  const proofText = await proofRegion.innerText();
  check("a verified model shows the patch level, GPU evidence, signing key and challenge, and what the result does not prove", proofText.includes("Verified") && proofText.includes("UpToDate") && proofText.includes("NVIDIA GPU evidence verified") && proofText.includes("0xsignerAAA") && proofText.includes("nonce777") && proofText.includes("What this does not show") && proofText.includes("does not prove that a particular answer"));
  await axeCheck(page, "model proof panel");
  await page.route("**/api/route/gateway", (route) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ verified: true, gateway: { signer: "gwkey1", tcb: "UpToDate", advisories: [], compose_hash: "c1", tls_bound: true, tls_fingerprint: "fp", nonce: "gwnonce9" }, means: ["The certificate used by this very connection is the one the verified software vouched for."], does_not_mean: ["It does not prove what the gateway did with a particular request."] }) }));
  await page.getByRole("button", { name: "Check the gateway (front door)" }).click();
  const gwRegion = page.getByRole("region", { name: "Gateway proof result" });
  await gwRegion.waitFor();
  const gwText = await gwRegion.innerText();
  check("the gateway proof says whether this connection's encryption ends inside the verified software and what it does not prove", gwText.includes("Verified") && gwText.includes("encryption ends inside the verified software") && gwText.includes("gwnonce9") && gwText.includes("does not prove what the gateway did"));
  await axeCheck(page, "gateway proof panel");
  await page.getByLabel("Model name").fill("bad/model");
  await page.getByRole("button", { name: "Check now" }).click();
  await page.getByRole("alert").filter({ hasText: "refuses it" }).waitFor();
  check("a refused model shows the reason, no verified badge and no deployment details", (await proofRegion.count()) === 0 && (await page.getByText("0xsignerAAA").count()) === 0);
  await page.getByText("Add pages from the web").click();
  await page.getByLabel("Web addresses, one per line (https only)").fill("https://docs.rs/tokio\nhttp://insecure.example/\nhttps://127.0.0.1/admin");
  const originsBefore = requestOrigins.size;
  await page.getByRole("button", { name: "Show what would be requested" }).click();
  await page.getByText("These exact addresses would be requested:").waitFor();
  const reviewText = await page.getByRole("region", { name: "Review before fetching" }).innerText();
  check("the research review lists the exact addresses, refuses the insecure and internal ones with a reason, and says nothing was sent", reviewText.includes("https://docs.rs/tokio") && (reviewText.match(/not allowed/g) ?? []).length === 2 && reviewText.includes("Nothing has been sent yet"));
  check("showing the review made no request to anywhere but the local service", requestOrigins.size === originsBefore);
  await page.getByRole("button", { name: "Cancel" }).click();
  check("cancelling the review discards it", (await page.getByText("These exact addresses would be requested:").count()) === 0);
  await page.getByLabel("Your message").fill("Where are login sessions kept?");
  await page.getByLabel("Your message").press("Shift+Enter");
  await page.getByLabel("Your message").type("(one line more)");
  check("Shift+Enter adds a line instead of sending", (await page.locator(".msg").count()) === 0);
  await page.getByLabel("Your message").dispatchEvent("keydown", { key: "Enter", isComposing: true, keyCode: 229, bubbles: true, cancelable: true });
  await page.waitForTimeout(300);
  check("Enter that confirms an input-method composition does not send the message", (await page.locator(".msg").count()) === 0);
  await page.getByLabel("Your message").press("Enter");
  await page.getByText("note(s) used").first().waitFor();
  await page.waitForLoadState("load");
  const reply = await page.locator(".msg.assistant").innerText();
  check("Enter sends and an answer appears marked as a test model with the notes used", reply.includes("[test model]") && reply.includes("Test model") && /1 note\(s\) used/.test(reply), reply.replace(/\n/g, " | ").slice(0, 160));
  await axeCheck(page, "chat screen");
  await page.screenshot({ path: join(shots, "13-chat.png") });
  await page.getByRole("button", { name: "Clear" }).click();
  check("Clear forgets the conversation", (await page.locator(".msg").count()) === 0);
  // a kept conversation survives a reload, can be opened again, and is gone after Forget
  await page.getByRole("button", { name: "Keep this conversation" }).click();
  await page.getByText("Kept", { exact: true }).waitFor();
  await page.getByLabel("Your message").fill("Remember this keptcanary line please");
  await page.getByLabel("Your message").press("Enter");
  await page.getByText("Saved in the encrypted store").waitFor();
  await page.waitForLoadState("load");
  await page.reload();
  await page.getByRole("link", { name: "Chat" }).click();
  await page.getByRole("heading", { name: "Chat", exact: true }).waitFor();
  await page.getByRole("button", { name: "Open" }).first().waitFor();
  check("after a reload the kept conversation is listed and nothing else is shown until it is opened", (await page.locator(".msg").count()) === 0 && (await page.getByRole("button", { name: "Open" }).count()) === 1);
  await page.getByRole("button", { name: "Open" }).click();
  await page.locator(".msg").nth(1).waitFor();
  check("opening the kept conversation brings back both sides of the exchange", (await page.locator(".msg").count()) === 2);
  await page.getByRole("button", { name: "Forget", exact: true }).click();
  check("Forget asks for a second press before removing anything", (await page.getByRole("button", { name: "Press again to forget" }).count()) === 1 && (await page.locator(".msg").count()) === 2);
  await page.getByRole("button", { name: "Press again to forget" }).click();
  await page.getByRole("button", { name: "Keep this conversation" }).waitFor();
  check("after Forget the conversation is gone from the list and the screen", (await page.getByRole("button", { name: "Open" }).count()) === 0 && (await page.locator(".msg").count()) === 0);
  // documents: an answer whose quotes are checked against the files
  await page.getByRole("link", { name: "Documents" }).click();
  await page.getByRole("heading", { name: "Ask your documents", exact: true }).waitFor();
  await page.getByRole("button", { name: "contracts" }).click();
  await page.getByLabel("Your question").fill("How long is the term?");
  await page.getByRole("button", { name: "Ask", exact: true }).click();
  await page.getByRole("heading", { name: "Answer", exact: true }).waitFor();
  const ans = await page.locator("#h-answer").locator("xpath=..").innerText();
  check("the answer says it is not proof and shows one quote found with its line and one not found", ans.includes("not proof") && ans.includes("1 of 2 quote(s) found") && ans.includes("Found in the file (line 1)") && ans.includes("Not found: the file does not contain this text"), ans.replace(/\n/g, " | ").slice(0, 200));
  check("the secret file is reported as not sent and its content is nowhere", ans.includes("Not sent to the model") && ans.includes(".env") && !ans.includes("sk_live"));
  const showBtn = page.getByRole("button", { name: "Show in file" });
  check("only a quote that was found offers to be shown in its file", (await showBtn.count()) === 1);
  await showBtn.click();
  const passage = page.locator("pre.passage");
  await passage.waitFor();
  check("the passage opens in place with line numbers and the cited line marked, and never shows the secret file", (await passage.locator(".hit").count()) === 1 && /\b1\b/.test(await passage.locator(".ln").first().innerText()) && !(await passage.innerText()).includes("sk_live"));
  await axeCheck(page, "documents screen with a passage open");
  await page.getByRole("button", { name: "Hide" }).click();
  check("Hide closes the passage", (await passage.count()) === 0);
  await axeCheck(page, "documents screen");
  await page.screenshot({ path: join(shots, "14-documents.png") });
  // activity: the signed log lists what happened, downloads, and verifies offline with the CLI
  await page.getByRole("link", { name: "Activity" }).click();
  await page.getByRole("heading", { name: "Activity", exact: true }).waitFor();
  await page.getByRole("cell", { name: "Delivery finished" }).first().waitFor();
  const rows = await page.locator("tbody tr").allInnerTexts();
  check("the activity list shows approvals, delivery and memory changes in plain words", ["Release approved", "Delivery finished", "Memory forgotten", "Review finished", "Suggestion accepted"].every((w) => rows.join("\n").includes(w)));
  check("the activity page never shows memory text or file contents", !rows.join("\n").includes("in-process dictionary") && !rows.join("\n").includes("euros"));
  await axeCheck(page, "activity screen");
  await page.screenshot({ path: join(shots, "12-activity.png") });
  const [logDl] = await Promise.all([page.waitForEvent("download"), page.getByRole("link", { name: "Download the signed log" }).click()]);
  const logPath = join(work, "event-log.json");
  await logDl.saveAs(logPath);
  const ok = spawnSync(kaku, ["log", "verify", logPath], { encoding: "utf8" });
  check("the downloaded log verifies offline with kaku log verify", ok.status === 0 && /VALID: \d+ entries/.test(ok.stdout), ok.stdout.slice(0, 120));
  const logText = readFileSync(logPath, "utf8");
  writeFileSync(logPath + ".bad", logText.replace("job.finished", "job.finishes"));
  const bad = spawnSync(kaku, ["log", "verify", logPath + ".bad"], { encoding: "utf8" });
  check("a changed log is reported INVALID by the CLI", bad.status === 1 && /INVALID/.test(bad.stdout), bad.stdout.slice(0, 120));
  await page.locator('.list a[href^="#/job/"]').first().click();
  await page.getByRole("tab", { name: "Release" }).click();
  await page.screenshot({ path: join(shots, "06-delivered.png"), fullPage: true });
  const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("link", { name: "Download the capsule" }).click()]);
  const capsulePath = join(work, "review.kkg");
  await download.saveAs(capsulePath);
  const cli = spawnSync(kaku, ["verify", capsulePath], { encoding: "utf8" });
  check("kaku verify accepts the delivered capsule", cli.status === 0 && /^VALID/m.test(cli.stdout) && /Not established/.test(cli.stdout));

  const v = await ctx.newPage();
  v.on("console", (m) => m.type() === "error" && problems.push("verify console: " + m.text()));
  v.on("pageerror", (e) => problems.push("verify pageerror: " + e.message));
  await v.goto(base + "/verify");
  await v.getByRole("heading", { name: "Check a capsule" }).waitFor();
  await v.setInputFiles("#file", capsulePath);
  await v.getByRole("heading", { name: "Valid" }).waitFor();
  check("the in-browser checker accepts it and lists what is not established", /Not established/.test(await v.locator("main").innerText()));
  await v.screenshot({ path: join(shots, "07-verify-valid.png"), fullPage: true });
  const bytes = readFileSync(capsulePath);
  const mark = Buffer.from('"repro-1.py":"');
  const at = bytes.lastIndexOf(mark) + mark.length;
  const tampered = Buffer.from(bytes);
  tampered[at] = tampered[at] === 65 ? 66 : 65;
  const tamperedPath = join(work, "tampered.kkg");
  writeFileSync(tamperedPath, tampered);
  await v.setInputFiles("#file", tamperedPath);
  await v.getByRole("heading", { name: "Not valid" }).waitFor();
  check("the in-browser checker rejects a tampered capsule with a reason", /changed|does not match/i.test(await v.locator("main").innerText()));
  await v.screenshot({ path: join(shots, "08-verify-tampered.png"), fullPage: true });
  const cliBad = spawnSync(kaku, ["verify", tamperedPath], { encoding: "utf8" });
  check("kaku verify rejects the tampered capsule", cliBad.status === 1 && /INVALID/.test(cliBad.stdout));

  const mobile = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "dark" });
  const mp = await mobile.newPage();
  await mp.goto(base + "/verify");
  await mp.getByRole("heading", { name: "Check a capsule" }).waitFor();
  await mp.screenshot({ path: join(shots, "09-mobile-dark-verify.png"), fullPage: true });
  const overflow = await mp.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
  check("verify page has no horizontal scroll on a phone", !overflow);
  await mobile.close();
} catch (e) {
  check("journey completed", false, String(e).slice(0, 400));
  await page.screenshot({ path: join(shots, "failure.png"), fullPage: true }).catch(() => {});
}
const unreach = await browser.newContext();
const up = await unreach.newPage();
await up.route("**/api/session", (r) => r.abort());
await up.goto(base + "/");
await up.getByRole("alert").waitFor({ timeout: 15000 }).catch(() => {});
check("when the local service cannot be reached the page says so and offers to retry instead of asking for a pairing code", /could not reach/i.test(await up.locator("body").innerText()) && (await up.getByRole("button", { name: "Try again" }).count()) === 1 && (await up.getByRole("heading", { name: "Pair this browser" }).count()) === 0);
await unreach.close();
check("every request the workspace made went to its own local service and nowhere else", [...requestOrigins].every((o) => o === base), JSON.stringify([...requestOrigins]));
check("zero console errors, page errors, failed requests or 5xx responses", problems.length === 0, problems.join(" | ").slice(0, 400));
await browser.close();
server.kill();
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} checks passed`);
process.exit(failed ? 1 : 0);
