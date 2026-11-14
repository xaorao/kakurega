import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdirSync, rmSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/extension-tmp");
const shots = join(repo, "target/e2e-shots");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
mkdirSync(shots, { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const extDir = join(repo, "apps/extension");
let bad = 0;
const check = (n, ok, d = "") => { if (!ok) bad++; console.log(`${ok ? "PASS" : "FAIL"}  ${n}${d ? "  " + d : ""}`); };

const data = join(work, "data");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", data, "--root", join(work, "root"), "--scripted", join(here, "script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: "ignore" });
for (let i = 0; i < 100 && !existsSync(join(data, "admin.json")); i++) await new Promise((r) => setTimeout(r, 100));
const admin = JSON.parse(readFileSync(join(data, "admin.json"), "utf8"));
const base = `http://127.0.0.1:${admin.port}`;
const adminFetch = async (path, method = "GET", body) => {
  const r = await fetch(base + path, { method, headers: { Authorization: `Bearer ${admin.token}`, ...(body ? { "content-type": "application/json" } : {}) }, body: body ? JSON.stringify(body) : undefined });
  return r.json();
};

// a hostile page on another loopback port that tries to reach the extension channel and the extension itself
const hostile = createServer((req, res) => {
  res.setHeader("content-type", "text/html");
  res.end(`<!doctype html><title>hostile</title><body><pre id="out">running</pre><script>
    const base = ${JSON.stringify(base)};
    const out = {};
    const tryit = async (name, fn) => { try { out[name] = await fn(); } catch (e) { out[name] = "blocked: " + e.name; } };
    (async () => {
      await tryit("authorised GET", async () => (await fetch(base + "/ext/status", { headers: { Authorization: "Bearer guess" } })).status);
      await tryit("simple POST capture", async () => { const r = await fetch(base + "/ext/capture", { method: "POST", mode: "no-cors", headers: { "content-type": "text/plain" }, body: JSON.stringify({ text: "planted by a web page", page: "https://evil.example/" }) }); return r.type; });
      await tryit("pair with a guessed code", async () => { const r = await fetch(base + "/ext/pair", { method: "POST", mode: "no-cors", headers: { "content-type": "text/plain" }, body: JSON.stringify({ code: "000000000000" }) }); return r.type; });
      await tryit("chrome runtime", async () => typeof chrome !== "undefined" && chrome.runtime && chrome.runtime.sendMessage ? "has sendMessage" : "no chrome.runtime");
      await tryit("window globals from the extension", async () => Object.keys(window).filter((k) => /kakurega/i.test(k)).join(",") || "none");
      document.getElementById("out").textContent = JSON.stringify(out);
    })();
  </script></body>`);
});
await new Promise((r) => hostile.listen(0, "127.0.0.1", r));
const hostileUrl = `http://127.0.0.1:${hostile.address().port}/`;

const ctx = await chromium.launchPersistentContext(join(work, "profile"), {
  executablePath: chrome,
  headless: true,
  args: [`--disable-extensions-except=${extDir}`, `--load-extension=${extDir}`, "--headless=new"],
});
try {
  let [sw] = ctx.serviceWorkers();
  if (!sw) sw = await ctx.waitForEvent("serviceworker", { timeout: 20000 });
  const extId = new URL(sw.url()).host;
  check("the extension loads in real Chrome with its own origin", /^[a-p]{32}$/.test(extId), extId);
  for (let i = 0; i < 50; i++) {
    if (await sw.evaluate(() => typeof chrome !== "undefined" && !!chrome.runtime && typeof chrome.runtime.getManifest === "function").catch(() => false)) break;
    await new Promise((r) => setTimeout(r, 200));
  }
  const manifest = await sw.evaluate(() => chrome.runtime.getManifest());
  check("the loaded manifest asks only for contextMenus and storage and the loopback host", JSON.stringify(manifest.permissions.sort()) === '["contextMenus","storage"]' && JSON.stringify(manifest.host_permissions) === '["http://127.0.0.1/*"]');
  const menus = await sw.evaluate(() => new Promise((res) => chrome.contextMenus.create({ id: "probe", title: "probe" }, () => res(chrome.runtime.lastError ? chrome.runtime.lastError.message : "created"))));
  check("the context-menu API is available to the worker", menus === "created" || /duplicate/i.test(menus), menus);

  const popup = await ctx.newPage();
  const errors = [];
  popup.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  popup.on("pageerror", (e) => errors.push(e.message));
  await popup.goto(`chrome-extension://${extId}/popup.html`);
  await popup.getByText("Not connected.").waitFor();
  check("an unpaired popup says so and shows the pairing form", await popup.locator("#pair").isVisible());
  await popup.screenshot({ path: join(shots, "ext-01-unpaired.png") });

  await popup.fill("#port", String(admin.port));
  await popup.fill("#code", "ffffffffffff");
  await popup.getByRole("button", { name: "Connect" }).click();
  await popup.locator("#error").filter({ hasText: "wrong, used or expired" }).waitFor();
  check("a wrong code is refused with a plain message", true);
  const code = (await adminFetch("/admin/extension/pairing", "POST", {})).code;
  await popup.fill("#code", code);
  await popup.getByRole("button", { name: "Connect" }).click();
  await popup.getByText(/Connected\./).waitFor();
  check("the right code pairs the extension and the pairing form goes away", (await popup.locator("#connected").isVisible()) && !(await popup.locator("#pair").isVisible()));
  await popup.screenshot({ path: join(shots, "ext-02-paired.png") });
  check("the service records which extension is paired", (await adminFetch("/admin/extension")).origin === `chrome-extension://${extId}`);
  const stored = await sw.evaluate(() => chrome.storage.local.get(null));
  check("the token is kept in extension-local storage only", typeof stored.token === "string" && stored.port === admin.port);

  // the capture module is the code the context-menu click runs; service workers cannot import() dynamically, so it is driven from an extension page (same origin, same fetch)
  const outcome = await popup.evaluate(async (selection) => (await import("./capture.js")).captureSelection(selection), { selectionText: "Acme's fiscal year ends in March", pageUrl: "https://wiki.example.com/page?secret=1" });
  check("a selection is queued and the message says it still needs review", outcome.ok === true && /Queued in Kakurega.*Review it there/.test(outcome.message), JSON.stringify(outcome));
  const refused = await popup.evaluate(async (selection) => (await import("./capture.js")).captureSelection(selection), { selectionText: "the password is hunter2hunter2", pageUrl: "https://wiki.example.com/" });
  check("a secret selection is refused by the service and reported", refused.ok === false, refused.message);

  // the owner sees it in the workspace as an untrusted suggestion and nothing is saved yet
  const app = await ctx.newPage();
  const code2 = (await adminFetch("/admin/pairing", "POST", {})).code;
  await app.goto(`${base}/#pair=${code2}`);
  await app.getByRole("link", { name: "Memory" }).click();
  await app.getByRole("heading", { name: "Memory", exact: true }).waitFor();
  const card = app.getByRole("region", { name: "From the browser" });
  await card.waitFor();
  check("the capture waits in the Memory screen, marked untrusted, naming only the site", (await card.innerText()).includes("fiscal year") && (await card.innerText()).includes("from untrusted content") && (await card.innerText()).includes("https://wiki.example.com") && !(await card.innerText()).includes("secret=1"));
  check("nothing is a memory before the owner accepts", (await app.locator(".mem").count()) === 0);
  await app.screenshot({ path: join(shots, "ext-03-queue.png") });
  await card.getByRole("button", { name: "Remember" }).click();
  await app.locator(".mem").first().waitFor();
  check("accepting turns it into an untrusted memory", (await app.locator(".mem").first().innerText()).includes("from untrusted content"));
  await popup.reload();
  await popup.getByText(/0 captures waiting/).waitFor();
  check("the popup shows the queue is empty again", true);

  // hostile page
  const evil = await ctx.newPage();
  await evil.goto(hostileUrl);
  await evil.waitForFunction(() => document.getElementById("out").textContent !== "running", null, { timeout: 15000 });
  const result = JSON.parse(await evil.locator("#out").innerText());
  check("a web page cannot read the extension channel (CORS blocks the authorised request)", String(result["authorised GET"]).startsWith("blocked"), result["authorised GET"]);
  check("a web page has no chrome.runtime to message the extension with", result["chrome runtime"] === "no chrome.runtime", result["chrome runtime"]);
  check("the extension injects nothing into pages", result["window globals from the extension"] === "none");
  const pending = (await (await fetch(`${base}/ext/status`, { headers: { Authorization: `Bearer ${stored.token}`, Origin: `chrome-extension://${extId}` } })).json()).pending_captures;
  check("a no-cors POST from a web page planted nothing (the origin is not an extension)", pending === 0, `pending=${pending}`);
  check("and a guessed pairing code from a web page did not pair anything new", (await adminFetch("/admin/extension")).origin === `chrome-extension://${extId}`);

  // disconnect and revoke
  await popup.getByRole("button", { name: "Disconnect this browser" }).click();
  await popup.getByText("Not connected.").waitFor();
  const afterForget = await popup.evaluate(async (selection) => (await import("./capture.js")).captureSelection(selection), { selectionText: "after disconnect", pageUrl: "https://a.example/" });
  check("after disconnecting, sending is refused locally", afterForget.ok === false && /Not paired/.test(afterForget.message));
  // found in real use: Disconnect used to clear only the extension's own storage, and Kakurega went on saying an
  // extension was paired
  const side = await adminFetch("/admin/extension");
  check("disconnecting ends the pairing on Kakurega's side too", side.paired === false || side.origin === null || side.origin === undefined, JSON.stringify(side));
  const lock = await adminFetch("/admin/extension/revoke", "POST", {});
  check("and the owner's own revoke then finds nothing left to revoke", lock.revoked === false);
  const unexpected = errors.filter((e) => !/status of (401|422)/.test(e)); // the wrong code and the secret selection are answered with these on purpose
  check("no unexpected console or page errors in the popup", unexpected.length === 0, unexpected.join(" | "));
} finally {
  await ctx.close();
  hostile.close();
  server.kill();
}
console.log(bad === 0 ? "\nextension checks passed" : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
