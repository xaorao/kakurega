// The screen matrix of the QA guide (Part 4.5, section SCREEN): every route of the web workspace in desktop and phone sizes,
// light and dark, English and Hindi, with real content in it. For each cell it records what a machine can judge (the number of
// interactive elements, sideways overflow, targets too small to press, accessibility violations, console errors, failed
// requests) and saves a screenshot for a person to read. It judges nothing about looks; the screenshots are for eyes.
//
//   node tests/e2e/audit_matrix.mjs [--quick]     writes target/audit/matrix.json and target/audit/shots/*.png

import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

// --empty: a fresh install with nothing in it, to see every page's empty state
const empty = process.argv.includes("--empty");
const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/audit-tmp");
const out = join(repo, empty ? "target/audit-empty" : "target/audit");
const shots = join(out, "shots");
const kaku = join(repo, "target/debug/kaku");
const quick = process.argv.includes("--quick");
rmSync(work, { recursive: true, force: true });
rmSync(out, { recursive: true, force: true });
mkdirSync(shots, { recursive: true });
const root = join(work, "root");
const project = join(root, "payments-service");
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
mkdirSync(join(root, "contracts"), { recursive: true });
writeFileSync(join(root, "contracts/agreement.md"), "The term of this agreement is twelve (12) months from the Effective Date.\nEither party may terminate with sixty (60) days notice.\n");
const data = join(work, "data");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const axeSource = readFileSync(join(here, "node_modules/axe-core/axe.min.js"), "utf8");

const server = spawn(kaku, ["serve", "--data-dir", data, "--root", root, "--scripted", join(here, "script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const base = new URL(link).origin;
const stop = () => { try { server.kill(); } catch {} };
process.on("exit", stop);

// real content: a finished review, a kept conversation with a turn, one saved note
const run = (args) => spawnSync(kaku, [...args, "--data-dir", data], { encoding: "utf8", timeout: 120000 });
const reviewed = empty ? { stdout: "" } : run(["review", project, "--for", "acme-audit", "--json"]);
const jobId = /"job":"(job-[\w-]+)"/.exec(reviewed.stdout)?.[1] ?? /(job-[\w]+)/.exec(reviewed.stdout)?.[1];
if (!empty) run(["chat", "Where are login sessions kept?", "--keep", "--json"]);
const base_url = base;

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const routes = [
  ["new-review", "#/"], ["waiting", "#/inbox"], ["documents", "#/docs"], ["chat", "#/chat"], ["memory", "#/memory"],
  ["instructions", "#/instructions"], ["skills", "#/skills"], ["meeting", "#/meeting"], ["break", "#/break"], ["settings", "#/settings"], ["privacy", "#/privacy"], ["agent-access", "#/agent"], ["activity", "#/activity"],
  ["projects", "#/projects"], ["code", "#/code"], ["roundtable", "#/roundtable"], ["team", "#/team"], ["handoff", "#/handoff"],
  ...(jobId ? [["review", `#/job/${jobId}`]] : []),
];
const viewports = quick ? [["desktop", 1280, 860]] : [["desktop", 1280, 860], ["phone", 390, 844]];
const schemes = quick ? ["dark"] : ["light", "dark"];
const langs = quick ? ["en"] : ["en", "hi"];
const rows = [];
let first = true;
for (const [vpName, width, height] of viewports) for (const scheme of schemes) for (const lang of langs) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: scheme, reducedMotion: "reduce" });
  const page = await ctx.newPage();
  const problems = [];
  page.on("console", (m) => { if (["error", "warning"].includes(m.type())) problems.push(`console ${m.type()}: ${m.text().slice(0, 160)}`); });
  page.on("pageerror", (e) => problems.push(`pageerror: ${String(e).slice(0, 160)}`));
  // a request the page cancels because the next route replaced it is the harness moving on, not a failure of the product
  page.on("requestfailed", (r) => { if (r.failure()?.errorText === "net::ERR_ABORTED") return; problems.push(`requestfailed: ${r.url().slice(0, 100)} ${r.failure()?.errorText ?? ""}`); });
  page.on("response", (r) => { if (r.status() >= 500) problems.push(`status ${r.status()}: ${r.url().slice(0, 100)}`); });
  // a pairing link works once: each browser context pairs with a new one
  const ctxLink = first ? link : /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(run(["pair"]).stdout)?.[0];
  await page.goto(ctxLink);
  await page.waitForSelector("header", { timeout: 20000 });
  // skip the welcome so the routes show their own content
  await page.evaluate(() => { try { localStorage.setItem("kk-welcome", "done"); } catch {} });
  await page.evaluate((l) => { try { localStorage.setItem("kk-lang", l); } catch {} }, lang);
  // the language is read when the page starts, so the page starts again
  await page.reload();
  await page.waitForSelector("header", { timeout: 20000 });
  for (const [name, hash] of routes) {
    problems.length = 0;
    await page.goto(`${base_url}/${hash}`);
    await page.waitForLoadState("load");
    await page.waitForTimeout(900);
    const cell = await page.evaluate(() => {
      const visible = (e) => { const r = e.getBoundingClientRect(); const s = getComputedStyle(e); return r.width > 0 && r.height > 0 && s.visibility !== "hidden" && s.display !== "none"; };
      const sel = "a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=tab],[role=link],[role=checkbox],[tabindex]:not([tabindex='-1'])";
      const els = [...document.querySelectorAll(sel)].filter(visible);
      const small = els.filter((e) => { const r = e.getBoundingClientRect(); return (r.width < 24 || r.height < 24) && !e.closest("[aria-hidden=true]"); }).map((e) => { const r = e.getBoundingClientRect(); const lab = (e.getAttribute("aria-label") || e.labels?.[0]?.textContent || e.textContent || "").trim().slice(0, 28); return `${e.tagName.toLowerCase()}${e.type ? "[" + e.type + "]" : ""} ${Math.round(r.width)}x${Math.round(r.height)} "${lab}"`; });
      const wide = [...document.querySelectorAll("body *")].filter((e) => { const r = e.getBoundingClientRect(); return r.right > innerWidth + 1 && r.width > 0 && visible(e) && !e.closest("pre,code,table,.codeview,.tree,[style*='overflow']"); }).slice(0, 3).map((e) => e.tagName.toLowerCase() + "." + String(e.className).split(" ")[0]);
      // copy: a count of exactly one followed by a plural noun ("1 entries"), or an unfilled {placeholder}
      const text = document.body.innerText;
      const copy = [...(text.match(/(^|\s)1 (entries|files|notes|passages|messages|hours|bytes|reviews|replies|findings|items|turns)\b/g) ?? []), ...(text.match(/\{\w+\}/g) ?? [])].map((x) => x.trim());
      return { copy, interactive: els.length, smallTargets: small.length, smallNames: small.slice(0, 12), overflowX: document.documentElement.scrollWidth > innerWidth + 1, wide, lang: document.documentElement.lang, title: document.title, h1: document.querySelector("h1")?.textContent?.trim() ?? "" };
    });
    // evaluate, not a script tag: the page forbids inline scripts, as it should
    await page.evaluate(axeSource).catch(() => {});
    const axe = await page.evaluate(async () => { try { const r = await window.axe.run(document, { runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"] } }); return r.violations.map((v) => `${v.id} x${v.nodes.length}`); } catch (e) { return ["axe failed: " + e]; } });
    const file = `${name}__${vpName}__${scheme}__${lang}.png`;
    await page.screenshot({ path: join(shots, file), fullPage: true }).catch(() => {});
    for (const c of cell.copy) problems.push("copy: " + c);
    rows.push({ route: name, viewport: vpName, scheme, lang, ...cell, axe, problems: [...problems], shot: file });
    first = false;
  }
  await ctx.close();
}
await browser.close();
stop();
writeFileSync(join(out, "matrix.json"), JSON.stringify(rows, null, 1));
const bad = rows.filter((r) => r.overflowX || r.axe.length || r.problems.length || (r.viewport === "phone" && r.smallTargets > 0));
console.log(`cells: ${rows.length}; with something to look at: ${bad.length}`);
for (const r of bad) console.log(`${r.route} ${r.viewport} ${r.scheme} ${r.lang}: ${[r.overflowX ? "OVERFLOW " + r.wide.join(",") : "", r.axe.length ? "axe " + r.axe.join(",") : "", r.problems.length ? r.problems.slice(0, 2).join(" ; ") : "", r.viewport === "phone" && r.smallTargets ? `small targets ${r.smallTargets} (${r.smallNames.join(", ")})` : ""].filter(Boolean).join(" | ")}`);
