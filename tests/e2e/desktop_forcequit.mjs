import "./quiet_crash.mjs";
import { spawnSync } from "node:child_process";
import { _electron as electron } from "playwright-core";
import { mkdirSync, rmSync, cpSync, renameSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// A desktop app that is killed outright (a crash, a forced quit, a power cut) cannot tell its companion to stop. The companion
// must notice by itself and leave, so no service is left running with nobody holding its window.
const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = process.env.KK_WORK ?? join(repo, "target/desktop-forcequit");
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
const companionPids = () => spawnSync("pgrep", ["-f", `serve --data-dir ${data}`], { encoding: "utf8" }).stdout.trim();
let app;
try {
  app = await electron.launch({
    executablePath: process.env.ELECTRON_BIN ?? join(repo, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"),
    args: [...(linux ? ["--no-sandbox"] : ["--use-mock-keychain"]), process.env.DESKTOP_APP_DIR ?? join(repo, "apps/desktop")],
    env: { ...process.env, KAKU_BIN: kaku, KAKUREGA_DATA_DIR: data, KAKU_ARGS: `--root ${root} --scripted ${join(here, "script.json")} --ui-dir ${join(repo, "apps/web/dist")}` },
  });
  const page = await app.firstWindow();
  await page.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 30000 });
  check("the workspace opens and its companion is running", companionPids() !== "" && existsSync(join(data, "admin.json")));
  // while the app is alive the same companion keeps running (it must not leave just because it read its secret)
  const before = companionPids();
  await sleep(5000);
  check("while the app is open the same companion keeps running", before !== "" && companionPids() === before, `${before} -> ${companionPids()}`);
  const electronPid = app.process().pid;
  process.kill(electronPid, "SIGKILL");
  let gone = false;
  const started = Date.now();
  for (let i = 0; i < 120 && !gone; i++) {
    await sleep(500);
    gone = companionPids() === "";
  }
  check("after the desktop app is killed outright the companion leaves by itself", gone, `after ${((Date.now() - started) / 1000).toFixed(1)} s`);
  check("and takes its admin file with it, so no stale token is left", !existsSync(join(data, "admin.json")));
} finally {
  await app?.close().catch(() => {});
  spawnSync("pkill", ["-9", "-f", `serve --data-dir ${data}`]);
}
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} force-quit checks passed` : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
