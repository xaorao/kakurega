// A free try on the deployed site, as a visitor with no key: one real question through the relay (it spends a little of
// the site's own key; run by hand).   SITE_URL=https://... node tests/e2e/site_free_live.mjs
import { chromium } from "playwright-core";
import { homedir } from "node:os";
import { join } from "node:path";
if (!process.env.SITE_URL) throw new Error("usage: SITE_URL=https://... node tests/e2e/site_free_live.mjs");
const U = process.env.SITE_URL.replace(/\/$/, "");
const b = await chromium.launch({ executablePath: process.env.CHROME ?? join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"), headless: true });
const p = await (await b.newContext({ viewport: { width: 1280, height: 1000 } })).newPage();
const problems = []; const sent = []; const hosts = new Set();
p.on("console", (m) => { if (m.type() === "error") problems.push(m.text().slice(0, 160)); });
p.on("pageerror", (e) => problems.push(e.message.slice(0, 160)));
p.on("request", (r) => { hosts.add(new URL(r.url()).host); sent.push((r.postData() ?? "") + " " + JSON.stringify(r.headers())); });
const check = (c, w) => { console.log(c ? "PASS" : "FAIL", w); if (!c) process.exitCode = 1; };
await p.goto(U + "/try/");
check(/3 free tries left on our key/.test(await p.locator("[data-tries]").innerText()), "a new visitor is offered three free tries");
await p.getByRole("button", { name: "Withhold, check, ask" }).click();
await p.locator('[data-step="sign"][data-state="ok"], [data-step="sign"][data-state="warn"], [data-state="bad"]').first().waitFor({ timeout: 180000 });
const states = await p.locator("[data-step]").evaluateAll((rows) => rows.map((r) => r.getAttribute("data-step") + ":" + r.getAttribute("data-state")).join(" "));
check(states === "withhold:ok proof:ok ask:ok sign:ok", "with no key of their own: withheld, proof checked, answered, signed by the model's own key: " + states);
const answer = await p.locator('[data-step="ask"] .answer').innerText().catch(() => "");
check(answer.trim().length > 40, "a real answer came back: " + answer.slice(0, 110).replace(/\s+/g, " "));
check(/2 free tries left/.test(await p.locator("[data-tries]").innerText()), "and one try is counted");
// the question goes through this site's relay; the client also fetches public proof material from NEAR and NVIDIA (no key goes with it)
check([...hosts].every((h) => [new URL(U).host, "cloud.near.ai", "nras.attestation.nvidia.com"].includes(h)) && !hosts.has("cloud-api.near.ai"), "the question went through this site's relay, never straight to NEAR's API: " + [...hosts].join(", "));
const all = sent.join("\n");
check(!/AKIA|sk_li|sk-[a-f0-9]{20}/.test(all) && !/Bearer sk-/.test(all), "neither the sample's keys nor any NEAR key is in anything the page sent");
check(!all.includes("def login(user)"), "the code left the page sealed, not as readable text");
check(problems.length === 0, "no console errors: " + problems.join(" | "));
await p.screenshot({ path: "target/site-free-live.png", fullPage: true });
await b.close();
