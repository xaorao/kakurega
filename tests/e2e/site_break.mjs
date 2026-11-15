// The break-it page as a visitor uses it, served as plain files with the host's headers: the product's own detector (the
// WebAssembly build) withholds a known key shape, a password in plain words gets through and the report link carries it
// filled in, withheld lines stay withheld in the report, the signed benchmark's numbers are the signed file's, the public
// count renders from GitHub's answer and falls back to a plain sentence when GitHub will not answer, nothing typed leaves
// the page, and it reads at two sizes in both themes. GitHub's API is answered by the test (page.route), so the run needs
// no network.
//   node tests/e2e/site_break.mjs      screenshots in target/site-break/   (SITE_REBUILD=0 uses the dist/ already built)

import { chromium } from "playwright-core";
import { createServer } from "node:http";
import { spawnSync } from "node:child_process";
import { createHash, createPublicKey, verify } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/site-break");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
const dir = join(repo, "apps/site/dist");
if (!existsSync(join(dir, "break/index.html")) || process.env.SITE_REBUILD !== "0") {
  const built = spawnSync("node", [join(repo, "apps/site/build.mjs")], { encoding: "utf8" });
  if (built.status !== 0) throw new Error(built.stderr);
}
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

// 1. the numbers: worked out here from the signed file, independently of the build
const env = JSON.parse(readFileSync(join(repo, "benchmarks/leak/results.signed.json"), "utf8"));
const payload = Buffer.from(env.payload, "base64url");
const pub = readFileSync(join(repo, "benchmarks/leak/signer.pub"), "utf8").trim();
const pae = Buffer.concat([Buffer.from(`DSSEv1 ${Buffer.byteLength(env.payloadType)} ${env.payloadType} ${payload.length} `), payload]);
check(verify(null, pae, createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: pub }, format: "jwk" }), Buffer.from(env.signatures[0].sig, "base64url")), "the leak benchmark's signature checks out against signer.pub");
const bench = JSON.parse(payload.toString("utf8"));
const group = (g) => bench.cases.filter((c) => c.group === g);
const leaks = (g) => group(g).filter((c) => c.verdict === "LEAK").length;
const want = { known: `${leaks("known shapes")} / ${group("known shapes").length}`, beyond: `${leaks("beyond the detector")} / ${group("beyond the detector").length}` };
check(want.known === "0 / 126" && want.beyond === "10 / 42", `the signed results hold ${want.known} known shapes and ${want.beyond} others reaching the model`);

// 2. the check of "got through" is the benchmark's own: every form scripts/bench-leak.py counts as a leak, and no other
const { forms, reportUrl } = await import(pathToFileURL(join(repo, "apps/site/break.js")).href);
const samples = ["purple elephant", "Tuesday-maple-42", "482913", "Blue42Harbor&517", "a much longer made-up secret here", "ünïcödé-pässwörd-1234", "spaces and / slashes ? = & + %"];
const py = spawnSync("python3", ["-I", "-c", `import importlib.util, json, sys
spec = importlib.util.spec_from_file_location("bench", sys.argv[1]); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
print(json.dumps({s: m.needles(s) for s in json.loads(sys.argv[2])}))`, join(repo, "scripts/bench-leak.py"), JSON.stringify(samples)], { encoding: "utf8" });
const theirs = py.status === 0 ? JSON.parse(py.stdout) : null;
check(theirs && samples.every((s) => JSON.stringify(forms(s).map((f) => f.text).sort()) === JSON.stringify([...theirs[s]].sort())), "the page counts as getting through exactly the forms the benchmark counts as a leak: " + (theirs ? samples.length + " secrets compared" : py.stderr.slice(-300)));

// 3. the report form: the page fills in exactly the fields the template has, and the template adds the label
const template = readFileSync(join(repo, ".github/ISSUE_TEMPLATE/break-it.yml"), "utf8");
const ids = [...template.matchAll(/^\s+id: ([a-z-]+)$/gm)].map((m) => m[1]);
const sample = new URL(reportUrl({ sent: "x", secret: "purple elephant", kind: "a password", lines: [1], detector: 3 }).url);
const filled = [...sample.searchParams.keys()].filter((k) => !["template", "title"].includes(k));
check(/^labels: \["break-it"\]$/m.test(template) && /^name: /m.test(template) && /^description: /m.test(template) && filled.every((k) => ids.includes(k)) && ["text", "secret", "kind"].every((k) => ids.includes(k)) && sample.searchParams.get("template") === "break-it.yml" && !sample.searchParams.has("labels"), `the issue form has the fields the link fills (${filled.join(", ")}) and adds the label itself; the link asks for no label (GitHub answers 404 to one a visitor may not set)`);
check(/made up/i.test(template) && /required: true/.test(template.split("id: made-up")[1] ?? ""), "the form says the secret must be made up, and a box saying so must be ticked");
// a long text still gives an address GitHub accepts, holding the line with the secret
const long = Array.from({ length: 200 }, (_, i) => `ligne ${i}: déjà vu, encore une fois, rien à voir ici`).join("\n") + "\nthe staging password is purple elephant";
const longUrl = reportUrl({ sent: long, secret: "purple elephant", kind: "a password", lines: [201], detector: 3 });
check(longUrl.shortened && longUrl.url.length <= 7000 && new URL(longUrl.url).searchParams.get("text").includes("purple elephant"), `a long text gives a ${longUrl.url.length}-character address that still holds the secret's line`);

// 4. the other pages lead here
for (const [file, href] of [["index.html", 'href="break/"'], ["try/index.html", 'href="../break/"'], ["proof/index.html", 'href="../break/"'], ["404.html", 'href="/break/"']]) check(readFileSync(join(dir, file), "utf8").includes(href), `${file} links to the break-it page`);
check(/\/break\/ : /.test(readFileSync(join(dir, "llms.txt"), "utf8")) && readFileSync(join(dir, "llms.txt"), "utf8").includes(`${want.known.split(" / ")[0]} of ${want.known.split(" / ")[1]} known shapes`), "llms.txt describes the page, with the signed numbers");

// the site, served with the headers the host will send
const types = { ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript", ".json": "application/json", ".png": "image/png", ".webp": "image/webp", ".woff2": "font/woff2", ".wasm": "application/wasm", ".kkg": "application/octet-stream" };
const rules = JSON.parse(readFileSync(join(dir, "vercel.json"), "utf8")).headers.map((h) => ({ re: new RegExp("^" + h.source + "$"), headers: h.headers }));
const policies = rules.filter((r) => r.re.test("/break/")).flatMap((r) => r.headers.filter((h) => h.key === "Content-Security-Policy").map((h) => h.value));
check(policies.length === 1 && /connect-src 'self' https:\/\/api\.github\.com;/.test(policies[0]) && /script-src 'self' 'wasm-unsafe-eval';/.test(policies[0]), "the page's policy lets it reach this site and GitHub's API only, and it is the only policy sent for it");
const srv = createServer((q, r) => {
  const path = q.url.split("?")[0];
  let p = join(dir, decodeURIComponent(path));
  if (!p.startsWith(dir)) { r.statusCode = 400; return r.end(); }
  if (existsSync(p) && statSync(p).isDirectory()) p = join(p, "index.html");
  if (!existsSync(p) || path.startsWith("/api/") || path === "/vercel.json" || path === "/package.json") { r.statusCode = 404; return r.end("not found"); }
  r.setHeader("content-type", types[extname(p)] ?? "application/octet-stream");
  for (const rule of rules) if (rule.re.test(path)) for (const h of rule.headers) r.setHeader(h.key, h.value);
  r.end(readFileSync(p));
}).listen(0, "127.0.0.1");
await new Promise((r) => srv.on("listening", r));
const origin = `http://127.0.0.1:${srv.address().port}`;
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });

const API = "https://api.github.com/search/issues";
const issue = (n, ...labels) => ({ number: n, labels: ["break-it", ...labels].map((name) => ({ name })) });
const json = (body, status = 200) => (route) => route.fulfill({ status, contentType: "application/json", headers: { "access-control-allow-origin": "*" }, body: JSON.stringify(body) });

/** One visitor: its own browser context, GitHub's API answered by `github`, every request and console line recorded. */
async function visit(name, viewport, scheme, github) {
  const ctx = await browser.newContext({ viewport, colorScheme: scheme });
  const page = await ctx.newPage();
  const log = { errors: [], pageErrors: [], requests: [], asked: [] };
  page.on("console", (m) => { if (m.type() === "error") log.errors.push(m.text().slice(0, 200)); });
  page.on("pageerror", (e) => log.pageErrors.push(e.message.slice(0, 200)));
  page.on("request", (r) => log.requests.push({ url: r.url(), body: r.postData() ?? "" }));
  await page.route((url) => url.origin !== origin, (route) => {
    const url = route.request().url();
    if (url.startsWith(API + "?")) { log.asked.push(url); return github(route, url); }
    return route.abort();
  });
  await page.goto(origin + "/break/");
  await page.locator("[data-line]").filter({ hasText: /^(Held|Got through)\.$/ }).waitFor({ timeout: 15000 });
  await page.locator("[data-reports-line]").filter({ hasText: /GitHub just now|could not be read/ }).waitFor({ timeout: 15000 });
  return { ctx, page, log, name };
}
// parts of the page rise in as they are scrolled to: scroll through once so a whole-page picture shows every part
const reveal = async (page) => {
  await page.evaluate(async () => {
    for (let y = 0; y < document.body.scrollHeight; y += 300) { scrollTo({ top: y, behavior: "instant" }); await new Promise((r) => setTimeout(r, 40)); }
    scrollTo({ top: 0, behavior: "instant" });
  });
  await page.waitForTimeout(900);
};
const verdictOf = async (page) => ({ line: await page.locator("[data-line]").innerText(), sub: await page.locator("[data-sub]").innerText(), act: await page.locator("[data-act]").isVisible() });
const outside = (log) => log.requests.map((r) => new URL(r.url)).filter((u) => u.origin !== origin && u.origin !== "https://api.github.com").map((u) => u.href);
const noSideways = (page) => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);
const gh = json({ total_count: 3, incomplete_results: false, items: [issue(1, "got-through"), issue(2, "held"), issue(3)] });

// 5. the challenge, at two sizes and both themes
for (const [name, viewport, scheme] of [["desktop-light", { width: 1440, height: 900 }, "light"], ["desktop-dark", { width: 1440, height: 900 }, "dark"], ["phone-light", { width: 390, height: 844 }, "light"], ["phone-dark", { width: 390, height: 844 }, "dark"]]) {
  const { ctx, page, log } = await visit(name, viewport, scheme, gh);
  const out = page.locator("[data-out]");
  let v = await verdictOf(page);
  check(v.line === "Held." && !v.act && !(await out.innerText()).includes("sk_li") && /withheld · holds a stripe secret key/.test(await out.innerText()), `${name}: the first example, a key in a settings file, is withheld and held, with no report offered`);
  check((await page.locator(".tally").first().innerText()).includes(want.known) && (await page.locator(".tally").first().innerText()).includes(want.beyond), `${name}: the signed numbers on the page are the signed file's: ${want.known}, ${want.beyond}`);
  check((await page.locator('[data-n="through"]').innerText()) === "1" && (await page.locator('[data-n="held"]').innerText()) === "1" && (await page.locator('[data-n="reports"]').innerText()) === "3" && (await page.locator('[data-n="unchecked"]').innerText()) === "1" && /3 reports, 1 not checked yet/.test(await page.locator("[data-reports-line]").innerText()), `${name}: the live count renders GitHub's answer: 1 got through, 1 held, 3 reports, 1 not checked`);
  check(log.asked.length === 1 && new URL(log.asked[0]).searchParams.get("q") === "repo:xaorao/kakurega is:issue label:break-it", `${name}: one search asked of GitHub, for this repository's break-it issues: ${log.asked.map((u) => decodeURIComponent(new URL(u).search)).join(" ")}`);
  // a verdict's words keep the page's own type in every state (a class name shared with another page's rule once turned them mono)
  check(!/Mono/.test(await page.locator("[data-sub]").evaluate((n) => getComputedStyle(n).fontFamily)) && (await page.locator("[data-verdict]").evaluate((n) => getComputedStyle(n).paddingLeft)) === "22px", `${name}: the held verdict is set in the page's type, with no other page's style on it`);
  check(await page.locator("[data-detector]").innerText() === "3", `${name}: the page says which generation of the detector it runs`);
  check(await noSideways(page), `${name}: no sideways scroll with the first verdict`);
  await page.waitForTimeout(700);
  await page.screenshot({ path: join(work, `${name}-top.png`) });

  // a visitor writes their own: a password in plain words
  const box = page.locator("[data-text]");
  const secretBox = page.locator("[data-secret]");
  const madeUp = "violet harbour lantern";
  await box.fill(`deploy notes\nthe wifi password is ${madeUp}\nall done`);
  await secretBox.fill(madeUp);
  await page.locator("[data-kind]").selectOption("a password");
  v = await verdictOf(page);
  check(v.line === "Got through." && /your secret, as written, on line 2/.test(v.sub) && v.act, `${name}: a password in plain words gets through, said plainly: ${v.sub}`);
  check((await page.locator("[data-out] mark").allInnerTexts()).join("|") === madeUp, `${name}: the secret is marked where it would be sent`);
  const href = await page.locator("[data-report]").getAttribute("href");
  const u = new URL(href);
  check(u.origin === "https://github.com" && u.pathname === "/xaorao/kakurega/issues/new" && u.searchParams.get("template") === "break-it.yml" && u.searchParams.get("secret") === madeUp && u.searchParams.get("kind") === "a password" && u.searchParams.get("text").includes(`the wifi password is ${madeUp}`) && u.searchParams.get("detector") === "3" && (await page.locator("[data-report]").getAttribute("target")) === "_blank", `${name}: the report link opens GitHub's form with the text, the secret and its kind filled in`);
  check(await noSideways(page), `${name}: no sideways scroll with a report offered`);
  await page.locator("[data-verdict]").scrollIntoViewIfNeeded();
  await page.screenshot({ path: join(work, `${name}-through.png`) });
  await page.locator("[data-challenge]").screenshot({ path: join(work, `${name}-window.png`) });

  // a key withheld and a plain password let through, in one text: the report carries only what would be sent
  const aws = "AKIA" + "Q7XK2M5P" + "LD4RT6WZ";
  await box.fill(`[deploy]\naws_key = "${aws}"\nnote: the vault password is ${madeUp}`);
  v = await verdictOf(page);
  const text = new URL(await page.locator("[data-report]").getAttribute("href")).searchParams.get("text");
  check(v.line === "Got through." && /line 3/.test(v.sub) && !text.includes(aws) && text.includes("[line 2 was withheld on the user's computer: it holds an aws access key id]") && text.includes(madeUp), `${name}: lines the page withheld stay withheld in the report: ${text.split("\n")[1]}`);
  // the key alone is held
  await secretBox.fill(aws);
  v = await verdictOf(page);
  check(v.line === "Held." && !v.act && !(await out.innerText()).includes(aws), `${name}: a known key shape typed by hand is withheld and held`);

  if (name === "desktop-light") {
    // every state that is not a verdict says what to do
    await secretBox.fill("");
    check((await verdictOf(page)).line === "Name your secret.", "with no secret named, the page asks for it");
    await secretBox.fill("abc");
    check((await verdictOf(page)).line === "Use a longer secret.", "a secret of three characters is not judged");
    await secretBox.fill("not-in-the-text-99");
    check((await verdictOf(page)).line === "That secret is not in your text.", "a secret that is not in the text is not judged");
    await box.fill("");
    check((await verdictOf(page)).line === "Write some text first.", "an empty box asks for text");
    // a secret written in base64 is found in that form
    const b64 = Buffer.from(madeUp).toString("base64");
    await box.fill(`blob = ${b64}`);
    await secretBox.fill(madeUp);
    v = await verdictOf(page);
    const b64Out = await out.innerText();
    check((v.line === "Got through." && /in base64/.test(v.sub) && b64Out.includes(b64)) || (v.line === "Held." && !b64Out.includes(b64)), `a secret written in base64 is judged in that form: ${v.line} ${v.sub}`);
    // half of a long secret getting through counts as getting through
    const long = "Tangerine-Mosaic-Quartz-Ferry";
    await box.fill(`first part: ${long.slice(0, 14)}`);
    await secretBox.fill(long);
    v = await verdictOf(page);
    check(v.line === "Got through." && /first half of your secret/.test(v.sub), `half of a long secret is a leak, as in the benchmark: ${v.sub}`);
    // the examples: the two known limits get through, the two known shapes are held
    const outcomes = [];
    for (const label of ["A key in a settings file", "A password said as a password", "A password in plain words", "A code with no name"]) {
      await page.getByRole("button", { name: label }).click();
      outcomes.push((await verdictOf(page)).line);
    }
    check(outcomes.join(" ") === "Held. Held. Got through. Got through.", `the four examples: ${outcomes.join(" ")}`);
    // every link leads somewhere
    const links = await page.evaluate(() => [...document.querySelectorAll("a[href]")].map((a) => a.getAttribute("href")));
    const broken = [];
    for (const h of new Set(links)) {
      if (h.startsWith("#")) { if (!(await page.locator(h).count())) broken.push(h); continue; }
      if (/^https:\/\/github\.com\/xaorao\/kakurega(\/|$)/.test(h)) continue;
      if (/^https?:/.test(h)) { broken.push("outside link " + h); continue; }
      const r = await fetch(new URL(h, origin + "/break/"));
      if (r.status !== 200) broken.push(h + " " + r.status);
    }
    check(broken.length === 0, `every link on the page leads somewhere (${new Set(links).size} links): ${broken.join(", ")}`);
  }
  await reveal(page);
  await page.screenshot({ path: join(work, `${name}-full.png`), fullPage: true });
  const typed = [madeUp, aws, "violet"];
  check(log.requests.every((r) => typed.every((s) => !r.url.includes(s) && !r.body.includes(s) && !r.url.includes(encodeURIComponent(s)))), `${name}: nothing typed was in any request the page made (${log.requests.length} requests)`);
  check(outside(log).length === 0, `${name}: no request went anywhere but this site and api.github.com: ${outside(log).slice(0, 3).join(", ")}`);
  check(log.errors.length === 0 && log.pageErrors.length === 0, `${name}: no console errors: ${[...log.errors, ...log.pageErrors].join(" | ")}`);
  await ctx.close();
}

// 6. the count when GitHub holds more reports than one answer lists: the two labels are asked for separately
{
  const { ctx, page, log } = await visit("many", { width: 1280, height: 900 }, "light", (route, url) => {
    const q = new URL(url).searchParams.get("q");
    if (q.endsWith(" -label:got-through")) return json({ total_count: 40, incomplete_results: false, items: [] })(route);
    if (q.endsWith(" label:got-through")) return json({ total_count: 7, incomplete_results: false, items: [] })(route);
    return json({ total_count: 250, incomplete_results: false, items: Array.from({ length: 100 }, (_, i) => issue(i + 1)) })(route);
  });
  const n = async (k) => page.locator(`[data-n="${k}"]`).innerText();
  check((await n("reports")) === "250" && (await n("through")) === "7" && (await n("held")) === "40" && (await n("unchecked")) === "203" && log.asked.length === 3, `with 250 reports the counts come from three searches: ${await n("through")} got through, ${await n("held")} held, ${await n("unchecked")} not checked`);
  check(log.errors.length === 0 && log.pageErrors.length === 0, "and no console errors");
  await ctx.close();
}

// 7. GitHub will not answer: rate-limited, unreachable, an answer that is not a count. The signed numbers stay, the live
//    count says in a sentence that it could not be read, and no status code or error text reaches the page.
for (const [name, github, scheme, viewport] of [
  ["rate-limited", json({ message: "API rate limit exceeded for 203.0.113.9. (But here's the good news: Authenticated requests get a higher rate limit.)", documentation_url: "https://docs.github.com/rest/overview/resources-in-the-rest-api#rate-limiting" }, 403), "light", { width: 1440, height: 900 }],
  ["too-many", json({ message: "secondary rate limit" }, 429), "dark", { width: 390, height: 844 }],
  ["unreachable", (route) => route.abort("internetdisconnected"), "light", { width: 390, height: 844 }],
  ["not-a-count", json({ items: "nope" }), "light", { width: 1280, height: 900 }],
  ["incomplete", json({ total_count: 2, incomplete_results: true, items: [issue(1)] }), "light", { width: 1280, height: 900 }],
]) {
  const { ctx, page, log } = await visit(name, viewport, scheme, github);
  const card = await page.locator("[data-reports]").innerText();
  const body = await page.locator("main").innerText();
  check(/The live count could not be read from GitHub just now\./.test(card) && !(await page.locator("[data-tally]").isVisible()) && !/\b(403|429)\b|rate limit|exceeded|Error|undefined|NaN/.test(card), `${name}: the live count falls back to a plain sentence with no code or error text`);
  check(body.includes(want.known) && body.includes(want.beyond), `${name}: the signed numbers are still shown`);
  check((await verdictOf(page)).line === "Held.", `${name}: the challenge works without GitHub`);
  // the browser itself notes a refused request; the page adds nothing of its own
  check(log.pageErrors.length === 0 && log.errors.every((e) => /^Failed to load resource/.test(e)), `${name}: no error from the page's own code: ${log.errors.join(" | ") || "none"}`);
  check(await noSideways(page), `${name}: no sideways scroll`);
  if (name === "rate-limited" || name === "too-many") {
    await page.locator("[data-reports]").scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(work, `${name}.png`) });
  }
  await ctx.close();
}

// 8. a browser that cannot run the detector says so and offers nothing it cannot back
{
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await ctx.newPage();
  await page.route("**/kk_secrets.wasm", (route) => route.fulfill({ status: 200, contentType: "application/wasm", body: Buffer.from("not wasm") }));
  await page.route((url) => url.origin !== origin, json({ total_count: 0, incomplete_results: false, items: [] }));
  await page.goto(origin + "/break/");
  await page.getByText("The detector could not be loaded in this browser.").waitFor({ timeout: 10000 });
  check(await page.locator("[data-text]").isDisabled() && !(await page.locator("[data-act]").isVisible()) && /No reports yet/.test(await page.locator("[data-reports-line]").innerText()), "with no detector the boxes are off, no report is offered, and the count still reads");
  await ctx.close();
}
await browser.close();
srv.close();
console.log("hash of the page checked:", createHash("sha256").update(readFileSync(join(dir, "break/index.html"))).digest("hex").slice(0, 16));
