import { spawnSync } from "node:child_process";
import { _electron as electron } from "playwright-core";
import { mkdirSync, rmSync, cpSync, renameSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = process.env.KK_WORK ?? join(repo, "target/desktop-recovery");
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
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const linux = process.platform === "linux";
const kaku = process.env.KAKU_BIN ?? join(repo, "target/debug/kaku");
const app = await electron.launch({
  executablePath: process.env.ELECTRON_BIN ?? join(repo, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"),
  args: [...(linux ? ["--no-sandbox"] : ["--use-mock-keychain"]), process.env.DESKTOP_APP_DIR ?? join(repo, "apps/desktop")],
  env: { ...process.env, KAKU_BIN: kaku, KAKUREGA_DATA_DIR: data, KAKU_ARGS: `--root ${root} --scripted ${join(here, "script.json")} --ui-dir ${join(repo, "apps/web/dist")}` },
});
const killCompanion = () => spawnSync("pkill", ["-9", "-f", `serve --data-dir ${data}`]);
const companionPid = () => spawnSync("pgrep", ["-f", `serve --data-dir ${data}`], { encoding: "utf8" }).stdout.trim();
const works = async (page) => {
  await page.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 30000 });
  const r = await page.evaluate(async () => (await fetch("/api/session")).status);
  return r === 200;
};
try {
  let page = await app.firstWindow();
  check("the workspace opens paired", await works(page));
  const firstPid = companionPid();
  const firstOrigin = new URL(page.url()).origin;

  killCompanion();
  await page.waitForURL((u) => new URL(u).origin !== firstOrigin, { timeout: 20000 }).catch(() => {});
  page = app.windows()[0];
  check("after the companion is killed it is started again and the window is back on a working, paired workspace", await works(page), `${firstOrigin} -> ${new URL(page.url()).origin}`);
  const secondPid = companionPid();
  check("exactly one new companion is running, not two", secondPid && secondPid !== firstPid && secondPid.split("\n").length === 1, `${firstPid} -> ${secondPid}`);

  await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0].webContents.forcefullyCrashRenderer());
  await sleep(3000);
  // Playwright keeps a crashed page handle crashed for good, so the reloaded window is read from the main process
  const state = await app.evaluate(async ({ BrowserWindow }) => {
    const wc = BrowserWindow.getAllWindows()[0].webContents;
    return wc.executeJavaScript(`new Promise(async (res) => { for (let i = 0; i < 60; i++) { if (document.querySelector("h1,h2") && document.body.innerText.includes("Review a project")) break; await new Promise(r => setTimeout(r, 250)); } const r = await fetch("/api/session"); res({ status: r.status, heading: document.body.innerText.includes("Review a project") }); })`);
  });
  check("after the window process crashes the window is reloaded onto a working workspace without a new login", state.status === 200 && state.heading, JSON.stringify(state));
  check("the companion was not restarted by a window crash", companionPid() === secondPid);

  // a part that keeps crashing is reported, not restarted forever
  const shown = [];
  await app.evaluate(({ dialog }) => {
    globalThis.__boxes = [];
    dialog.showErrorBox = (t, c) => globalThis.__boxes.push([t, c]);
  });
  for (let i = 0; i < 5; i++) {
    killCompanion();
    await sleep(i === 0 ? 4000 : 6000);
  }
  const boxes = await app.evaluate(() => globalThis.__boxes);
  shown.push(...boxes);
  check("a companion that keeps dying is eventually reported to the owner instead of being restarted forever", shown.some(([t]) => t === "Kakurega stopped"), JSON.stringify(shown).slice(0, 160));
  check("no companion is left running after the limit", companionPid() === "", companionPid());
} finally {
  await app.close().catch(() => {});
  killCompanion();
}
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} recovery checks passed` : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
