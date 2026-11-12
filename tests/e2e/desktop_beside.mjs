// The desktop app opened while a service started from a terminal already runs on the same data folder: the app does not
// bind a window to that service (which has no shell secret and may lack --require-approver), says what is running and
// what to do, leaves it untouched, and leaves no service of its own behind.
//   node tests/e2e/desktop_beside.mjs
import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/desktop-beside");
spawnSync("pkill", ["-f", `serve --data-dir ${join(work, "data")}`]);
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const project = join(root, "payments-service");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
const data = join(work, "data");
mkdirSync(data, { recursive: true });
const kaku = join(repo, "target/debug/kaku");
const results = [];
const check = (name, ok, detail = "") => {
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail && !ok ? "  " + detail : ""}`);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const servers = () => spawnSync("pgrep", ["-f", `serve --data-dir ${data}`], { encoding: "utf8" }).stdout.trim().split("\n").filter(Boolean);

// what the app shows and loads is written to a file by a small script loaded before the app's own code
const seen = join(work, "seen.jsonl");
const stub = join(work, "observe.cjs");
writeFileSync(stub, `
const { appendFileSync } = require("node:fs");
const note = (o) => appendFileSync(${JSON.stringify(seen)}, JSON.stringify(o) + "\\n");
// loaded before Electron has set itself up: it attaches as soon as Electron's own module answers
const attach = () => {
  let electron;
  try { electron = require("electron"); } catch { electron = null; }
  if (!electron || typeof electron !== "object" || !electron.dialog) return setTimeout(attach, 5);
  const { app, dialog } = electron;
  // a real error box holds the app until it is closed; this one holds it for three seconds the same way
  dialog.showErrorBox = (title, body) => { note({ box: title, body }); Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 3000); };
  app.on("browser-window-created", (_e, w) => { note({ window: true }); w.webContents.on("did-start-navigation", (d) => note({ load: d.url ?? String(d) })); });
  note({ attached: true });
};
attach();
`);

const foreign = spawn(kaku, ["serve", "--data-dir", data, "--root", root, "--scripted", join(here, "script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: "ignore" });
try {
  for (let i = 0; i < 150 && !existsSync(join(data, "admin.json")); i++) await sleep(100);
  const before = JSON.parse(readFileSync(join(data, "admin.json"), "utf8"));
  check("a service started from a terminal runs on the data folder", before.pid === foreign.pid, JSON.stringify(before));

  const electronBin = process.env.ELECTRON_BIN ?? join(repo, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron");
  const desktop = spawn(electronBin, [...(process.platform === "linux" ? ["--no-sandbox"] : ["--use-mock-keychain"]), join(repo, "apps/desktop")], {
    env: { ...process.env, KAKU_BIN: kaku, KAKUREGA_DATA_DIR: data, KAKU_ARGS: `--root ${root} --scripted ${join(here, "script.json")} --ui-dir ${join(repo, "apps/web/dist")}`, NODE_OPTIONS: `--require ${stub}` },
    stdio: "ignore",
  });
  const exited = await Promise.race([new Promise((r) => desktop.on("exit", () => r(true))), sleep(45000).then(() => false)]);
  if (!exited) desktop.kill("SIGKILL");
  const events = existsSync(seen) ? readFileSync(seen, "utf8").trim().split("\n").filter(Boolean).map((l) => JSON.parse(l)) : [];
  check("the observer was attached to the app before anything was shown", events.length > 0 && events[0].attached === true, JSON.stringify(events));
  check("the app closes by itself instead of waiting on a service it cannot use", exited);
  const box = events.find((e) => e.box);
  check("it says another service runs for this data folder, names the process and says what to do", !!box && box.body.includes(`process ${foreign.pid}`) && /Quit it, then open the app again/.test(box.body), JSON.stringify(events));
  check("no window was opened, so nothing was shown from or sent to that service", !events.some((e) => e.window || e.load), JSON.stringify(events));
  check("only one error was shown, not one per restart attempt", events.filter((e) => e.box).length === 1, JSON.stringify(events));

  await sleep(1500);
  const after = JSON.parse(readFileSync(join(data, "admin.json"), "utf8"));
  check("the terminal's service is untouched: same process, same address, same token", after.pid === before.pid && after.port === before.port && after.token === before.token);
  const health = await fetch(`http://127.0.0.1:${after.port}/api/health`).then((r) => r.status).catch(() => 0);
  check("and it still answers", health === 200, String(health));
  check("the app left no service of its own running", servers().length === 1 && servers()[0] === String(foreign.pid), servers().join(","));
} finally {
  foreign.kill();
  await sleep(500);
  spawnSync("pkill", ["-f", `serve --data-dir ${data}`]);
}
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} side-by-side checks passed` : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
