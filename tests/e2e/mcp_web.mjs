// Outside tools as a person uses them: add a program in Instructions, check its tools, allow one, see the choice survive a
// reload, and see a bad path refused. Uses the scripted fake MCP server (`cargo build -p kk-mcpc --bins`) and `cargo build -p kk-cli`.
//
//   node tests/e2e/mcp_web.mjs     screenshots in target/mcp-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/mcp-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
const fake = join(repo, "target/debug/fake_mcp");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
process.on("exit", () => { try { server.kill(); } catch {} });
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1100 } })).newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error" && !/status of 422/.test(m.text())) problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => { if (!/ERR_ABORTED/.test(r.failure()?.errorText ?? "")) problems.push("requestfailed: " + r.url()); });
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { if (!cond) { console.log("FAIL", what); process.exitCode = 1; } else console.log("PASS", what); };
const origin = new URL(link).origin;

await page.goto(link);
await page.waitForSelector("nav");
await page.goto(origin + "/#/instructions");
await page.getByRole("heading", { name: "Outside tools" }).waitFor();
check(await page.getByText("No outside programs yet.").isVisible(), "a new install has no outside programs and says so");

await page.getByLabel("A short name for it").fill("docs");
await page.getByLabel("Full path of the program").fill("node");
await page.getByRole("button", { name: "Add program" }).click();
await page.getByRole("alert").first().waitFor();
check(/absolute path/.test(await page.getByRole("alert").first().innerText()), "a bare program name is refused in words");

await page.getByLabel("Full path of the program").fill(fake);
await page.getByLabel(/^Settings it needs/).fill("FAKE_MODE=annot\nAPI_KEY=hunter2-not-for-display");
await page.getByRole("button", { name: "Add program" }).click();
await page.getByText("Added. Check its tools next.").waitFor();
await shot("1-added");
check(await page.getByText("Settings: API_KEY, FAKE_MODE").isVisible(), "the program shows the names of its settings");
check(!(await page.locator("main").innerText()).includes("hunter2-not-for-display"), "a setting's value is never shown");

await page.getByRole("button", { name: "Check its tools" }).click();
await page.getByText("Checked: 3 tools.").waitFor();
await shot("2-checked");
const echo = page.getByLabel(/echo/);
check(!(await echo.isChecked()), "a listed tool starts off");
await echo.check();
await page.getByText("Allowed").first().waitFor();
check(await echo.isChecked(), "allowing a tool shows it as allowed");
await page.reload();
await page.goto(origin + "/#/instructions");
await page.getByRole("heading", { name: "Outside tools" }).waitFor();
check(await page.getByLabel(/echo/).isChecked(), "the choice is still there after a reload");
check(!(await page.getByLabel(/write/).isChecked()), "the other tool is still off");
check(await page.getByText("The server says it can change things.").isVisible(), "a tool the server says can change things is labelled so");
check(await page.getByText("The server does not say whether it changes things.").isVisible(), "a tool with no hint is labelled as not said");
const write = page.getByLabel(/write/).first();
check(await write.isDisabled(), "a tool that may change things cannot be allowed until the owner confirms they know");
await page.locator("#mcp-k-docs-write").check();
check(await write.isEnabled(), "after the confirmation it can be allowed");
await shot("3-allowed");

await page.getByRole("button", { name: "Remove" }).last().click();
await page.getByText("No outside programs yet.").waitFor();
check(true, "removing the program removes its tools too");
check(problems.length === 0, "no console errors: " + problems.join("; "));
await browser.close();
server.kill();
process.exit(process.exitCode ?? 0);
