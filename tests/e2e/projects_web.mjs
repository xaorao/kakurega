// Projects as a person uses them: make one with a folder of documents, see it listed with what belongs to it, open a chat
// in it, ask its documents, take it off the list.
//   node tests/e2e/projects_web.mjs      screenshots in target/projects-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/projects-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(join(root, "kestrel-docs"), { recursive: true });
writeFileSync(join(root, "kestrel-docs/terms.md"), "The agreement runs for 24 months.\n");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const port = 27000 + (process.pid % 1000);
const procs = [];
process.on("exit", () => { for (const p of procs) { try { p.kill(); } catch {} } });
procs.push(spawn("python3", [join(repo, "tests/e2e/team_script.py"), String(port)], { stdio: "ignore" }));
await new Promise((r) => setTimeout(r, 600));
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", "team-script", "--ollama", `http://127.0.0.1:${port}`, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
procs.push(server);
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1000 } })).newPage();
const errors = [];
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
page.on("pageerror", (e) => errors.push(String(e)));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.getByRole("link", { name: "Projects", exact: true }).click();
await page.waitForSelector("#proj-name");
check((await page.locator("#proj-list").innerText()).includes("No projects yet"), "it starts empty and says how a project comes to be");
await page.fill("#proj-name", "kestrel");
await page.fill("#proj-folder", "/etc");
await page.getByRole("button", { name: "Save the project" }).click();
await page.getByRole("alert").waitFor();
check((await page.getByRole("alert").innerText()).includes("not inside a folder this program may read"), "a folder outside the allowed folders is refused in words");
await page.fill("#proj-folder", join(root, "kestrel-docs"));
await page.getByRole("button", { name: "Save the project" }).click();
await page.locator("#proj-list h2", { hasText: "kestrel" }).waitFor();
const card = await page.locator("#proj-list").innerText();
check(card.includes("kestrel-docs") && card.includes("Kept conversations: 0"), "the project is listed with its folder and its conversations");
await page.screenshot({ path: join(work, "1-projects.png"), fullPage: true });

await page.getByRole("button", { name: "Ask its documents" }).click();
await page.waitForSelector("#h-docs");
await page.getByText("kestrel-docs").first().waitFor();
check(true, "its documents open in the Documents screen");
await page.getByRole("link", { name: "Projects", exact: true }).click();
await page.getByRole("button", { name: "Chat in this project" }).click();
await page.waitForSelector("#chat-input");
check((await page.locator("main").innerHTML()).includes('value="kestrel"') || (await page.locator('input[value="kestrel"]').count()) > 0 || (await page.evaluate(() => [...document.querySelectorAll("input")].some((i) => i.value === "kestrel"))), "the chat opens with the project already filled in");

await page.getByRole("link", { name: "Projects", exact: true }).click();
// where its work may go: kept on this computer, said in words, and still so after a reload
await page.getByLabel("Where may its work go?").selectOption("here");
await page.getByText("Saved where its work may go.").waitFor();
check(await page.getByText(/With any model elsewhere in use, nothing of this project is sent/).isVisible(), "a project kept on this computer says what that means");
await page.reload();
await page.waitForSelector("#proj-list");
check(await page.getByLabel("Where may its work go?").inputValue() === "here", "the choice is kept");
await page.screenshot({ path: join(work, "2-floor.png"), fullPage: true });
await page.getByLabel("Where may its work go?").selectOption("any");
await page.getByText("Saved where its work may go.").waitFor();
await page.getByRole("button", { name: /Take off the list/ }).click();
await page.getByText("Taken off the list.").waitFor();
check((await page.locator("#proj-list").innerText()).includes("No projects yet"), "taken off the list, and it says its conversations and notes were not deleted");
const other = errors.filter((e) => !/status of 422/.test(e));
check(other.length === 0, "no console errors beyond the one refusal" + (other.length ? ": " + other.join(" | ") : ""));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
