// hunt helper (temporary, deleted after the hunt): start kaku serve + a browser, then evaluate posted JS against them.
//   node _hunt_tools_repl.mjs NAME [kaku serve args...]
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
import http from "node:http";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/hunt-tools");
const [name, ...serveArgs] = process.argv.slice(2);
const shots = join(work, "shots");
mkdirSync(shots, { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const S = { problems: [], log: "", repo, work, shots };
S.startServer = async (args) => {
  S.log = "";
  const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--ui-dir", join(repo, "apps/web/dist"), ...args], { stdio: ["ignore", "pipe", "pipe"] });
  S.server = server;
  server.stdout.on("data", (d) => (S.log += d));
  server.stderr.on("data", (d) => (S.log += d));
  const until = Date.now() + 60000;
  while (!/#pair=/.test(S.log) && Date.now() < until) await new Promise((r) => setTimeout(r, 100));
  const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(S.log)?.[0];
  if (!link) throw new Error("no pairing link:\n" + S.log);
  S.link = link;
  S.base = new URL(link).origin;
  return link;
};
S.stopServer = async () => { if (S.server) { S.server.kill(); await new Promise((r) => setTimeout(r, 800)); } };
process.on("exit", () => { try { S.server?.kill(); } catch {} });
S.browser = await chromium.launch({ executablePath: chrome, headless: true });
S.attach = (page, tag = "") => {
  page.on("console", (m) => { if (m.type() === "error" || m.type() === "warning") S.problems.push(`${tag}console.${m.type()}: ${m.text().slice(0, 300)} @ ${m.location()?.url ?? ""}`); });
  page.on("pageerror", (e) => S.problems.push(`${tag}pageerror: ${e.message}`));
  page.on("requestfailed", (r) => S.problems.push(`${tag}requestfailed: ${r.url()} ${r.failure()?.errorText}`));
  page.on("response", (r) => { if (r.status() >= 400) S.problems.push(`${tag}status ${r.status()}: ${r.request().method()} ${r.url()}`); });
  page.on("dialog", (d) => { S.problems.push(`${tag}dialog(${d.type()}): ${d.message().slice(0, 200)}`); if (S.dialogAnswer !== "manual") (S.dialogAnswer === "dismiss" ? d.dismiss() : d.accept()).catch(() => {}); });
};
S.newPage = async (opts = {}) => {
  const ctx = await S.browser.newContext({ viewport: { width: 1440, height: 900 }, acceptDownloads: true, ...opts });
  const page = await ctx.newPage();
  S.attach(page);
  return { ctx, page };
};
S.shot = async (n, full = false, p = S.page) => { const path = join(shots, `${n}.png`); await p.screenshot({ path, fullPage: full }); return path; };
S.flat = (s) => s.replace(/\s+/g, " ").trim();
S.main = async (p = S.page) => S.flat(await p.locator("main").innerText());
S.take = () => { const p = S.problems; S.problems = []; return p; };
S.sleep = (ms) => new Promise((r) => setTimeout(r, ms));
S.mode = async (theme, width, p = S.page) => { await p.emulateMedia({ colorScheme: theme }); await p.setViewportSize({ width, height: width < 600 ? 844 : 900 }); };

await S.startServer(serveArgs);
const first = await S.newPage();
S.ctx = first.ctx; S.page = first.page;
await S.page.goto(S.link);
await S.page.waitForSelector("nav", { timeout: 30000 }).catch(() => {});

const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const srv = http.createServer(async (req, res) => {
  let body = "";
  req.on("data", (d) => (body += d));
  req.on("end", async () => {
    let out;
    try {
      const v = await new AsyncFunction("S", "page", body)(S, S.page);
      out = typeof v === "string" ? v : JSON.stringify(v, null, 1);
    } catch (e) {
      out = "ERROR: " + String(e?.stack ?? e).split("\n").slice(0, 4).join("\n");
    }
    res.end(out ?? "undefined");
  });
});
srv.listen(0, "127.0.0.1", () => {
  writeFileSync(join(work, `${name}.port`), String(srv.address().port));
  console.log("ready", S.base, srv.address().port);
});
