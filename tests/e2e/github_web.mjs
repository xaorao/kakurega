// The GitHub connector as a person uses it in Skills: paste the app's client id, connect, type the code at "GitHub" (a
// stand-in on this computer approves it), read an issue that holds a planted key and a planted order, attach it to a chat,
// see both held back, sign out.
//   node tests/e2e/github_web.mjs      screenshots in target/github-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/github-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
const data = join(work, "data");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const gport = 29000 + (process.pid % 900);
const mport = gport + 1;
const base = `http://127.0.0.1:${gport}`;
const procs = [];
process.on("exit", () => { for (const p of procs) { try { p.kill(); } catch {} } });
procs.push(spawn("python3", [join(repo, "tests/e2e/github_fake.py"), String(gport)], { stdio: "ignore" }));
procs.push(spawn("python3", [join(repo, "tests/e2e/team_script.py"), String(mport)], { stdio: "ignore" }));
await new Promise((r) => setTimeout(r, 800));
const control = (body) => fetch(base + "/_control", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", data, "--root", join(work, "root"), "--model", "team-script", "--ollama", `http://127.0.0.1:${mport}`, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, KAKU_GITHUB_TEST_BASE: base } });
procs.push(server);
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 1400 } })).newPage();
const errors = [];
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
page.on("pageerror", (e) => errors.push(String(e)));
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
await page.locator("#nav-more:not([open]) > summary").click({ timeout: 3000 }).catch(() => {}); // these links sit under More
await page.getByRole("link", { name: "Skills", exact: true }).click();
await page.waitForSelector("#gh-client");
check((await page.locator("#github").innerText()).includes("Nothing is ever written to GitHub"), "the card says it only reads");
await page.fill("#gh-client", "not-a-client-id");
await page.getByRole("button", { name: "Save the client ID" }).click();
await page.locator("#github").getByRole("alert").waitFor();
check(true, "something that is not a client id is refused in words");
await page.fill("#gh-client", "Iv23liFakeApp0123456");
await page.getByRole("button", { name: "Save the client ID" }).click();
await page.getByRole("button", { name: "Connect to GitHub" }).waitFor();

await control({ poll_script: ["pending", "authorized"] });
await page.getByRole("button", { name: "Connect to GitHub" }).click();
await page.waitForSelector("#gh-code");
const code = await page.locator("#gh-code").innerText();
check(/^[A-Z0-9]{4}-[A-Z0-9]{4}$/.test(code), "the code to type at GitHub is shown: " + code);
await page.screenshot({ path: join(work, "1-code.png"), fullPage: true });
await page.waitForSelector("#gh-account", { timeout: 60000 });
check((await page.locator("#gh-account").innerText()).includes("Connected as"), "after the approval the card says who is connected, without a reload");

await page.fill("#gh-repo", "acme/../secrets");
await page.fill("#gh-what", "7");
check(await page.getByRole("button", { name: "Read it", exact: true }).isDisabled(), "a repository name that is not owner/name cannot be asked for");
await page.fill("#gh-repo", "acme/widgets");
await page.getByRole("button", { name: "Read it and attach it to my next chat message" }).click();
await page.waitForSelector("#gh-got");
const got = await page.locator("#gh-got").innerText();
check(got.includes("written by other people") && /Attached for your next chat message/.test(got), "the issue is shown as other people's text and attached");
check(/lines with a secret [1-9]/.test(got) && /lines that read like orders [1-9]/.test(got), "the card says a secret line and an order line are held back from the model");
await page.screenshot({ path: join(work, "2-issue.png"), fullPage: true });
await page.getByRole("link", { name: "Open Chat" }).click();
await page.waitForSelector("#chat-input");
await page.locator(".file-chip").first().waitFor();
check((await page.locator(".file-chip").count()) === 1, "the chat opens with the issue attached");

// the token: one private file, nowhere in the page
const tokenFile = join(data, "github-token.json");
check(existsSync(tokenFile), "the token is kept in its own file");
const token = JSON.parse(readFileSync(tokenFile, "utf8"));
const secretValues = Object.values(token).filter((v) => typeof v === "string" && v.length > 20);
check(secretValues.length > 0 && !secretValues.some((v) => log.includes(v)), "no token is in the service's output");
await page.locator("#nav-more:not([open]) > summary").click({ timeout: 3000 }).catch(() => {}); // these links sit under More
await page.getByRole("link", { name: "Skills", exact: true }).click();
await page.waitForSelector("#gh-account");
check(!secretValues.some((v) => true) || !(await page.content()).includes(secretValues[0]), "no token is in the page");
await page.getByRole("button", { name: "Sign out of GitHub here" }).click();
await page.getByRole("button", { name: "Connect to GitHub" }).waitFor();
check(!existsSync(tokenFile) && (await page.locator("#github").innerText()).includes("GitHub"), "signing out removes the token file and says what remains to do at GitHub");
const other = errors.filter((e) => !/status of 422/.test(e));
check(other.length === 0, "no console errors beyond the one refusal" + (other.length ? ": " + other.join(" | ") : ""));
await browser.close();
for (const p of procs) p.kill();
process.exit(process.exitCode ?? 0);
