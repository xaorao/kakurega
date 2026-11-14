import { chromium } from "playwright-core";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
// run by hand: SITE_URL=https://... KAKU_TEST_NEAR_KEY_FILE=FILE node tests/e2e/site_try_live.mjs   (one short answer on NEAR)
if (!process.env.SITE_URL || !process.env.KAKU_TEST_NEAR_KEY_FILE) throw new Error("usage: SITE_URL=https://... KAKU_TEST_NEAR_KEY_FILE=FILE node tests/e2e/site_try_live.mjs");
const U = process.env.SITE_URL.replace(/\/$/, "");
const key = /NEAR_AI_API_KEY=(\S+)/.exec(readFileSync(process.env.KAKU_TEST_NEAR_KEY_FILE, "utf8"))[1];
const b = await chromium.launch({ executablePath: join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"), headless: true });
const p = await (await b.newContext({ viewport: { width: 1280, height: 1000 } })).newPage();
const problems = []; const sent = [];
p.on("console", (m) => { if (m.type() === "error") problems.push(m.text().slice(0, 160)); });
p.on("pageerror", (e) => problems.push(e.message.slice(0, 160)));
p.on("request", (r) => sent.push(new URL(r.url()).host + " " + (r.postData() ?? "").slice(0, 4000)));
const check = (c, w) => { console.log(c ? "PASS" : "FAIL", w); if (!c) process.exitCode = 1; };
// (whether free tries are on is another test: site_free_live.mjs)
await p.goto(U + "/try/");
// with a key typed in, the page talks to NEAR directly
await p.locator(".own-key summary").click();
await p.locator("[data-key]").fill(key);
await p.getByRole("button", { name: "Withhold, check, ask" }).click();
await p.waitForFunction(() => ["ok", "warn", "bad"].includes(document.querySelector('[data-step="sign"]')?.getAttribute("data-state")) || document.querySelector('[data-state="bad"]'), null, { timeout: 180000 });
const sign = await p.locator('[data-step="sign"]').innerText();
check(/Signed by the model's own hardware key, and the signature checks out/.test(sign), "live, with your own key: withheld, proof checked, answered, signature checked: " + (await p.locator('[data-step="ask"] .answer').innerText().catch(() => "")).replace(/\s+/g, " ").slice(0, 120));
const code = await p.locator("[data-code]").inputValue();
const secrets = [/AKIA[A-Z0-9]{16}/.exec(code)[0], /sk_live_[A-Za-z0-9]+/.exec(code)[0]];
check(secrets.every((s) => !sent.join("\n").includes(s)), "the withheld keys left the page in no request");
check(!sent.filter((s) => s.startsWith("kakurega-eight")).join("\n").includes(key), "the typed key was not sent to our site");
check(problems.filter((x) => !/503/.test(x)).length === 0, "no console errors beyond the one refusal: " + problems.join(" | "));
await p.screenshot({ path: "target/site-try/live.png", fullPage: true });
await b.close();
