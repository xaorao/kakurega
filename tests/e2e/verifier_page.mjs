// The standalone capsule check page, opened from disk with no server and no network: a valid capsule, a forged model
// receipt, a changed byte, a file that is not a capsule, a wrong and a right pinned key, a phone-width view and dark mode.
//
//   node tests/e2e/verifier_page.mjs     screenshots in target/verifier-page/

import { chromium } from "playwright-core";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
import { verifyCapsule } from "../../packages/verifier-js/src/index.js";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/verifier-page");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
const run = (cmd, args, cwd) => {
  const r = spawnSync(cmd, args, { cwd, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(" ")}: ${r.stderr}`);
};
run("node", ["build.mjs"], join(repo, "apps/verifier-page"));
run("cargo", ["run", "-q", "-p", "kk-evidence", "--example", "make_receipt_capsules", "--", work], repo);
const page_url = "file://" + join(repo, "apps/verifier-page/dist/kakurega-verify.html");
const two = join(work, "two.kkg");
const bytes = readFileSync(two);
const flipped = Buffer.from(bytes);
flipped[flipped.length - 20] ^= 1;
writeFileSync(join(work, "flipped.kkg"), flipped);
writeFileSync(join(work, "notes.txt"), "just some text\n");
const key = JSON.parse(bytes.toString()).keys[0];

// shares come from the shared vector file, so the page is held to the same cases as the two verifiers
const shareVectors = JSON.parse(readFileSync(join(repo, "fixtures/share/vectors.json"), "utf8")).vectors;
const shareFile = (name, file) => {
  const v = shareVectors.find((x) => x.name === name);
  if (!v) throw new Error("no share vector named " + name);
  writeFileSync(join(work, file), Buffer.from(v.share, "base64url"));
  return join(work, file);
};
const goodShare = shareFile("two files of five", "good.kks");
const changedShare = shareFile("content changed", "changed.kks");
const markupShare = shareFile("a file whose text looks like markup", "markup.kks");
const shareKey = JSON.parse(readFileSync(goodShare, "utf8")).keys[0];

const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1000, height: 900 } });
const problems = [];
const requests = [];
const page = await ctx.newPage();
page.on("console", (m) => { if (m.type() === "error") problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("request", (r) => { if (!r.url().startsWith("file:")) requests.push(r.url()); });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const verdict = async () => (await page.locator(".verdict").first().innerText()).trim();

await page.goto(page_url);
await page.setInputFiles("#file", two);
await page.locator(".verdict").waitFor();
check((await verdict()).startsWith("Valid"), "a good capsule is valid");
let body = await page.locator("main").innerText();
check(/Model signatures checked \(2\)/.test(body) && /vendor\/model/.test(body), "and both model signatures are listed");
check(/does not establish/i.test(body) && /attested confidential hardware/.test(body) && /answer is true/.test(body), "and it says what the check does not establish, hardware and truth included");
check(/no, this is only the key the capsule carries/.test(body), "and it says the signer is not pinned");
await page.screenshot({ path: join(work, "valid.png"), fullPage: true });

// ---- a share of some files of a release ----
await page.setInputFiles("#file", goodShare);
await page.getByText("Valid share").waitFor();
body = await page.locator("main").innerText();
check(/2 of the release's 5 files/.test(body) && /finding-01\.txt/.test(body) && /finding-04\.txt/.test(body), "a good share is valid and says how many of the release's files it holds");
check(/finding number 1: the token stays valid after logout/.test(body), "the shared file's text is shown");
check(!/finding-00\.txt|finding-02\.txt|finding-03\.txt/.test(body) && !/finding number [023]/.test(body), "nothing of the files that were not shared is shown");
check(/does not establish/i.test(body) && /other files/.test(body) && /Who the release was made for/.test(body), "and it says what a share does not establish");
check(/no, this is only the key the share carries/.test(body), "and that the signer is not pinned");
await page.screenshot({ path: join(work, "share-valid.png"), fullPage: true });
await page.fill("#pin", shareKey);
await page.getByText("yes, matches the key you pasted").waitFor();
check(true, "pinned to its signer it says so");
await page.fill("#pin", shareVectors.find((x) => x.name === "pinned to another key").pinned[0]);
await page.getByText("It was not signed by the key you pasted.").waitFor();
check((await verdict()) === "Not valid", "pinned to another key it is refused in words");
await page.fill("#pin", "");
await page.setInputFiles("#file", changedShare);
await page.getByText("is not part of the signed release").waitFor();
check((await verdict()) === "Not valid" && (await page.locator("pre").count()) === 0, "a share whose file was changed is refused, and shows no file");
await page.setInputFiles("#file", markupShare);
await page.getByText("Valid share").waitFor();
check((await page.evaluate(() => window.pwned)) === undefined && (await page.locator("main img, main script").count()) === 0 && /<script>window\.pwned=1<\/script>/.test(await page.locator("pre").first().innerText()), "markup inside a shared file is shown as text and does not run");

await page.setInputFiles("#file", join(work, "forged.kkg"));
await page.getByText("model receipt's signature does not match").waitFor();
check((await verdict()) === "Not valid", "a forged model receipt is refused, with the reason in words");
await page.screenshot({ path: join(work, "forged.png"), fullPage: true });

await page.setInputFiles("#file", join(work, "flipped.kkg"));
await page.locator(".verdict").waitFor();
check((await verdict()) === "Not valid", "a capsule with one changed byte is refused");
const flippedText = await page.locator("#out").innerText();
check(!/^Refused \([a-z_]+\)\.$/m.test(flippedText) && /[A-Z][^.]+\./.test(flippedText), "and the reason is given in words, never as a bare code: " + flippedText.split("\n")[1]?.slice(0, 70));

await page.setInputFiles("#file", join(work, "notes.txt"));
await page.getByText("The file is not a capsule").waitFor();
check(true, "a text file is refused as not a capsule");

await page.setInputFiles("#file", two);
await page.locator(".verdict").waitFor();
await page.fill("#pin", "A".repeat(43));
await page.getByText("Not valid").waitFor();
check((await verdict()) === "Not valid", "a capsule is refused when the expected key is a different key");
await page.fill("#pin", "not a key");
await page.getByText("That is not a signer key").waitFor();
check(true, "a malformed expected key is reported, not ignored");
await page.fill("#pin", key);
await page.getByText("matches the key you pasted").waitFor();
check((await verdict()).startsWith("Valid"), "the right expected key gives valid and says it is pinned");
// the signer line the page itself shows can be pasted back as the expected signer; a different one is refused
await page.fill("#pin", "");
await page.locator(".verdict").waitFor();
const shown = (await page.locator("dd.code").first().innerText()).trim();
check(/^ed25519:[0-9a-f]{64}$/.test(shown), `the signer is shown as a line that can be copied: ${shown.slice(0, 24)}`);
await page.fill("#pin", shown);
await page.getByText("matches the key you pasted").waitFor();
check((await verdict()).startsWith("Valid"), "the signer line the page shows is accepted as the expected signer");
await page.fill("#pin", "ed25519:" + "0".repeat(64));
await page.getByText("It was not signed by the key you pasted").waitFor();
check((await verdict()) === "Not valid", "a different signer line refuses the capsule");
// the key itself written in hex (what `kaku verify` prints as "signing key", and what NEAR stores) names the same signer
const hexKey = Buffer.from(key.replace(/-/g, "+").replace(/_/g, "/"), "base64").toString("hex");
for (const form of [hexKey, "ed25519:" + hexKey, hexKey.toUpperCase()]) {
  await page.fill("#pin", "");
  await page.locator(".verdict").waitFor();
  await page.fill("#pin", form);
  await page.getByText("matches the key you pasted").waitFor();
  check((await verdict()).startsWith("Valid"), `the key in hex is accepted as the expected signer (${form.slice(0, 12)}…)`);
}

await page.fill("#pin", "");
const late = await ctx.newPage();
await late.clock.setFixedTime(new Date("2035-01-01T00:00:00Z"));
await late.goto(page_url);
await late.setInputFiles("#file", two);
await late.locator(".verdict").waitFor();
check((await late.locator(".verdict").innerText()).trim() === "Not valid" && /expired/i.test(await late.locator("main").innerText()), "a capsule is refused as expired when the device clock is past its expiry");

check(requests.length === 0, "no network request was made: " + requests.join(", "));
check(problems.length === 0, "no console errors: " + problems.join("; "));

const phone = await (await browser.newContext({ viewport: { width: 375, height: 800 }, colorScheme: "dark" })).newPage();
await phone.goto(page_url);
await phone.setInputFiles("#file", two);
await phone.locator(".verdict").waitFor();
const overflow = await phone.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
check(!overflow, "phone width in dark mode has no sideways scroll");
await phone.screenshot({ path: join(work, "phone-dark.png"), fullPage: true });

// ---- the NEAR lookup: nothing is sent until asked; then only the digest and the keys go to the public RPC ----
const cctx = await browser.newContext({ viewport: { width: 1000, height: 1300 } });
const cp = await cctx.newPage();
const cproblems = [];
cp.on("console", (m) => { if (m.type() === "error" && !/Failed to load resource/.test(m.text())) cproblems.push("console: " + m.text()); });
cp.on("pageerror", (e) => cproblems.push("pageerror: " + e.message));
const calls = [];
const elsewhere = [];
let mode = "registered";
cp.on("request", (r) => { if (!r.url().startsWith("file:") && !r.url().startsWith("https://rpc.testnet.fastnear.com")) elsewhere.push(r.url()); });
await cctx.route("https://rpc.testnet.fastnear.com/**", async (route) => {
  const body = JSON.parse(route.request().postData());
  const args = JSON.parse(Buffer.from(body.params.args_base64, "base64").toString());
  calls.push({ method: body.params.method_name, account: body.params.account_id, args, headers: route.request().headers() });
  if (mode === "down") return route.fulfill({ status: 503, body: "busy" });
  if (mode === "notregistry") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ jsonrpc: "2.0", id: 1, error: { name: "HANDLER_ERROR" } }) });
  let result = null;
  const m = body.params.method_name;
  if (m === "get" && mode === "registered") result = { registered_by: "alice.testnet\u001b[2J", registered_at: "1700000000000000000", capsule_signer: args.capsule_signer, receipts: [], order: "4" };
  // the registry that keeps the receipts' digest: this capsule's own, or another's
  if (m === "get" && (mode === "digest" || mode === "digest-other")) result = { registered_by: "alice.testnet", registered_at: "1700000000000000000", capsule_signer: args.capsule_signer, receipts_sha256: mode === "digest" ? ownDigest : "00".repeat(32), receipt_count: 2, signers: [], order: null };
  if (m === "hardware" && mode === "registered" && calls.filter((c) => c.method === "hardware").length === 1) result = { signer: args.signer, tcb_status: "UpToDate", attested_at: "1700000100000000000", attested_by: "alice.testnet", compose_sha256: "cd".repeat(32), advisory_ids: [], mr_td: "ef".repeat(48) };
  if (m === "order") result = { state: "Paid", amount: "497090000000000000000000", fee: "9941800000000000000000", token: null, client: "carl.testnet", reviewer: "alice.testnet" };
  return route.fulfill({ status: 200, contentType: "application/json", headers: { "access-control-allow-origin": "*" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, result: { result: Array.from(Buffer.from(JSON.stringify(result))) } }) });
});
await cp.goto(page_url);
check((await cp.locator("#lookup").count()) === 0, "there is no NEAR lookup before a capsule is checked");
await cp.setInputFiles("#file", join(work, "forged.kkg"));
await cp.locator(".verdict").waitFor();
check((await cp.locator("#lookup").count()) === 0, "nor under a capsule that is not valid");
await cp.setInputFiles("#file", two);
await cp.locator("#lookup").waitFor();
check(calls.length === 0 && /sends this capsule's digest and the keys named in it/.test(await cp.locator("#chain").innerText()), "under a valid capsule the lookup is offered, says what it sends, and has sent nothing");
await cp.locator("#lookup").click();
await cp.getByText("Registered on NEAR").waitFor();
const chainText = await cp.locator("#chain").innerText();
const digest = createHash("sha256").update(bytes).digest("hex");
const signerHex = Buffer.from(key, "base64url").toString("hex");
const allowed = new Set(["capsule_sha256", "capsule_signer", "signer", "order_id"]);
check(calls.length === 4 && calls.every((c) => c.account === "kakurega-registry-c68af3.testnet" && Object.keys(c.args).every((k) => allowed.has(k))) && calls[0].args.capsule_sha256 === digest && calls[0].args.capsule_signer === signerHex, "the RPC was asked four things, with the digest, the signer key, two model keys and the order number and nothing else: " + JSON.stringify(calls.map((c) => [c.method, Object.keys(c.args)])));
check(calls.every((c) => !("cookie" in c.headers) && !("referer" in c.headers)), "with no cookie and no referrer");
check(/alice\.testnet\[2J/.test(chainText) && !chainText.includes("\u001b") && /2023-11-14 22:13:20 UTC/.test(chainText), "the registering account and the time are shown, with control characters from the chain removed");
check(/2 of this capsule's model signatures were not registered/.test(chainText), "model signatures the chain does not hold for this capsule are counted");
check(/Hardware record for model key/.test(chainText) && /Intel patch level then: UpToDate/.test(chainText) && /No hardware record at this registry for model key/.test(chainText), "a model key with a hardware record and one without are told apart");
check(/Registered for order 4/.test(chainText) && /0\.49709 NEAR/.test(chainText) && /State: Paid/.test(chainText), "the order it was registered for is shown in NEAR");
check(/Amount: 0\.49709 NEAR to the reviewer/.test(chainText) && /Fee on top: 0\.00994 NEAR, to the registry's fee account/.test(chainText), "the order's fee is shown apart from the reviewer's amount");
check(/does not establish/.test(chainText) && /claim in the capsule is true/.test(chainText) && /quote is recent/.test(chainText) && /GPU/.test(chainText), "and what the chain does not establish is said");
await cp.screenshot({ path: join(work, "chain-registered.png"), fullPage: true });

// a registry that keeps the digest of the receipts it verified: the page re-computes it from this capsule's own receipts
const report = await verifyCapsule(readFileSync(two), {});
const lines = report.receipts.map((r) => `${r.signer}:${r.model}:${r.requestSha256}:${r.responseSha256}:${r.signature}\n`).join("");
var ownDigest = createHash("sha256").update(lines).digest("hex");
check(report.receipts.length === 2 && report.receipts.every((r) => /^[0-9a-f]{128}$/.test(r.signature)), "the capsule's two receipts are read with their signatures");
mode = "digest";
await cp.locator("#lookup").click();
await cp.getByText("The contract verified 2 model signatures registered with it.").waitFor();
let digestText = await cp.locator("#chain").innerText();
check(!/not registered with it/.test(digestText) && !/not this capsule's own/.test(digestText), "every one of the capsule's signatures is matched by the registered digest");
mode = "digest-other";
await cp.locator("#lookup").click();
await cp.getByText("not this capsule's own").waitFor();
digestText = await cp.locator("#chain").innerText();
check(/The model signatures registered with it are not this capsule's own: their digest differs\./.test(digestText), "a registered digest that is not this capsule's is said plainly");
await cp.screenshot({ path: join(work, "chain-digest-other.png"), fullPage: true });

mode = "unregistered";
await cp.locator("#lookup").click();
await cp.getByText("Not registered on NEAR under this signing key").waitFor();
check(/nobody registered this exact file/.test(await cp.locator("#chain").innerText()), "an unregistered capsule is said to be unregistered, not invalid");
await cp.screenshot({ path: join(work, "chain-unregistered.png"), fullPage: true });
mode = "down";
await cp.locator("#lookup").click();
await cp.getByRole("alert").filter({ hasText: "answered 503" }).waitFor();
check((await cp.getByText("Registered on NEAR").count()) === 0, "a failing RPC is reported and no earlier result is left standing");
mode = "notregistry";
await cp.locator("#lookup").click();
await cp.getByRole("alert").filter({ hasText: "does not answer as a Kakurega registry" }).waitFor();
check(true, "an account that is not a registry is named as such");
const before = calls.length;
await cp.fill("#registry", "Not An Account");
await cp.locator("#lookup").click();
await cp.getByRole("alert").filter({ hasText: "not a NEAR account name" }).waitFor();
await cp.selectOption("#net", "mainnet");
check((await cp.inputValue("#registry")) === "", "no registry is assumed on mainnet");
await cp.locator("#lookup").click();
await cp.getByRole("alert").filter({ hasText: "has no registry on this network yet" }).waitFor();
check(true, "mainnet with no registry typed says there is none there yet, and that it is on testnet");
check(calls.length === before, "a name that is not an account is refused before anything is sent");
check(elsewhere.length === 0, "nothing was sent anywhere but the NEAR RPC: " + elsewhere.join(", "));
check(cproblems.length === 0, "no console errors in the lookup: " + cproblems.join("; "));

const cphone = await (await browser.newContext({ viewport: { width: 375, height: 900 }, colorScheme: "dark" })).newPage();
await cphone.goto(page_url);
await cphone.setInputFiles("#file", two);
await cphone.locator("#lookup").waitFor();
check(!(await cphone.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)), "the lookup section fits a phone in dark mode");
await cphone.screenshot({ path: join(work, "chain-phone-dark.png"), fullPage: true });

// a browser whose Web Crypto has no Ed25519 (older Safari and Chromium): the page still checks, through the fallback
const noEd = await browser.newContext({ viewport: { width: 1000, height: 900 } });
await noEd.addInitScript(() => {
  const real = crypto.subtle.importKey.bind(crypto.subtle);
  crypto.subtle.importKey = (format, key, algorithm, ...rest) =>
    (typeof algorithm === "string" ? algorithm : algorithm?.name) === "Ed25519"
      ? Promise.reject(new DOMException("Ed25519 is not supported", "NotSupportedError"))
      : real(format, key, algorithm, ...rest);
});
const ne = await noEd.newPage();
ne.on("pageerror", (e) => problems.push("pageerror: " + e.message));
await ne.goto(page_url);
await ne.setInputFiles("#file", two);
await ne.locator(".verdict").first().waitFor();
check((await ne.locator(".verdict").first().innerText()).trim().startsWith("Valid"), "in a browser without Ed25519 in Web Crypto, a good capsule is still valid");
await ne.setInputFiles("#file", join(work, "forged.kkg"));
await ne.getByText("Not valid").first().waitFor();
check((await ne.locator(".verdict").first().innerText()).trim() === "Not valid", "and the forged one is still refused");
await noEd.close();

// a browser that gives the page no Web Crypto at all: the page names the browser as the cause, not the file
const noCrypto = await browser.newContext({ viewport: { width: 1000, height: 900 } });
await noCrypto.addInitScript(() => Object.defineProperty(globalThis.crypto, "subtle", { value: undefined }));
const nc = await noCrypto.newPage();
await nc.goto(page_url);
await nc.setInputFiles("#file", two);
await nc.locator(".verdict").first().waitFor();
check(/does not give pages the Web Crypto functions/.test(await nc.locator(".verdict").first().innerText()) && /kaku verify/.test(await nc.locator(".verdict").first().innerText()), "with no Web Crypto at all, the page says the browser is the cause and names the command line");
await noCrypto.close();

// LIVE, only when asked: the capsule that tests/e2e/chain_register.py registered on testnet, read by the page itself
// (KAKU_LIVE_CAPSULE names another registered capsule, such as one a real NEAR review delivered)
const liveCapsule = process.env.KAKU_LIVE_CAPSULE || join(repo, "target/chain-register-tmp/review.kkg");
if (process.env.KAKU_LIVE_CHAIN === "1") {
  const { existsSync } = await import("node:fs");
  if (!existsSync(liveCapsule)) {
    check(false, "KAKU_LIVE_CHAIN=1 needs target/chain-register-tmp/review.kkg (run tests/e2e/chain_register.py with a testnet account first)");
  } else {
    const lp = await (await browser.newContext({ viewport: { width: 1000, height: 1200 } })).newPage();
    await lp.goto(page_url);
    await lp.setInputFiles("#file", liveCapsule);
    await lp.locator("#lookup").waitFor();
    await lp.locator("#lookup").click();
    await lp.locator("#chain-out .verdict").waitFor({ timeout: 30000 });
    const live = await lp.locator("#chain").innerText();
    check(/Registered by\s+kakurega-[a-z0-9-]+\.testnet/.test(live) && /kakurega-registry-c68af3\.testnet on NEAR testnet/.test(live), "LIVE: the page reads a real registration from NEAR testnet by itself");
    await lp.screenshot({ path: join(work, "chain-live.png"), fullPage: true });
  }
}
await browser.close();
