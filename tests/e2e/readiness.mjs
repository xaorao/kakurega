import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/readiness-tmp");
const shots = join(repo, "target/e2e-shots");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root/proj"), { recursive: true });
mkdirSync(shots, { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const axeSource = readFileSync(join(here, "node_modules/axe-core/axe.min.js"), "utf8");
let bad = 0;
const check = (n, ok, d = "") => { if (!ok) bad++; console.log(`${ok ? "PASS" : "FAIL"}  ${n}${d ? "  " + d : ""}`); };

// Nothing answers at the model address and no virtual machine of that name exists: every setup problem is real.
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", join(work, "root"), "--ollama", "http://127.0.0.1:9", "--vm", "kk-no-such-vm", "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let out = "";
server.stdout.on("data", (d) => (out += d));
server.stderr.on("data", (d) => (out += d));
const t = Date.now() + 30000;
while (!/#pair=/.test(out) && Date.now() < t) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(out)?.[0];
if (!link) { console.log("FAIL  kaku serve did not start:\n" + out); process.exit(1); }

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const problems = [];
try {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 }, colorScheme: "light" });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto(link);
  await page.getByRole("heading", { name: "Finish setup first" }).waitFor({ timeout: 20000 });
  for (const scheme of ["light", "dark"]) {
    await page.emulateMedia({ colorScheme: scheme });
    await new Promise((r) => setTimeout(r, 500)); // buttons animate their colours for 120 ms
    const text = await page.getByRole("region", { name: "Setup check" }).innerText();
    check(`[${scheme}] the screen names the model service problem and the exact fix`, /Local model service/.test(text) && /ollama serve/.test(text));
    check(`[${scheme}] the screen names the missing sandbox machine and the script that makes it`, /Sandbox virtual machine/.test(text) && /Run scripts\/provision-vm/.test(text) && /makes the sandbox machine kk-no-such-vm/.test(text));
    check(`[${scheme}] the worker is reported as not checked, not as a bogus path`, /not checked: the virtual machine is not running/.test(text));
    check(`[${scheme}] Start review stays disabled while setup is incomplete`, await page.getByRole("button", { name: "Start review" }).isDisabled());
    await page.screenshot({ path: join(shots, `readiness-${scheme}.png`) });
    await page.evaluate(axeSource);
    const res = await page.evaluate(async () => await window.axe.run(document, { runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "best-practice"] } }));
    const v = res.violations.filter((x) => ["serious", "critical", "moderate"].includes(x.impact));
    check(`[${scheme}] accessibility (axe)`, v.length === 0, v.map((x) => `${x.id} ${x.nodes[0]?.target?.join(" ")}`).join(" | "));
  }
  await ctx.close();
  check("no page errors", problems.length === 0, problems.join(" | "));
} finally {
  await browser.close();
  server.kill();
}
console.log(bad === 0 ? "\nreadiness checks passed" : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
