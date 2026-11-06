import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync, copyFileSync, openSync, readSync, writeSync, closeSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { packageApp } from "../../apps/desktop/scripts/package.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/packaged-test");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
// Tampered apps crash on purpose; macOS would show 'quit unexpectedly' for each. Hide those dialogs for the run only.
const crashKey = "com.apple.CrashReporter";
const before = spawnSync("defaults", ["read", crashKey, "DialogType"], { encoding: "utf8" });
const hadValue = before.status === 0;
spawnSync("defaults", ["write", crashKey, "DialogType", "none"]);
const restoreCrashDialogs = () => (hadValue ? spawnSync("defaults", ["write", crashKey, "DialogType", before.stdout.trim()]) : spawnSync("defaults", ["delete", crashKey, "DialogType"]));
process.on("exit", restoreCrashDialogs);
const results = [];
const check = (n, ok, d = "") => (results.push(ok), console.log(`${ok ? "PASS" : "FAIL"}  ${n}${d ? "  " + d : ""}`));

const pkg = await packageApp(join(work, "pkg"), { cookieEncryption: false });
check("the app is packaged as one asar with its integrity hash in Info.plist, and the load-only-from-asar and integrity fuses on", pkg.fuses["1"] === 48 && pkg.fuses["4"] === 49 && pkg.fuses["5"] === 49 && pkg.fuses["0"] === 48 && pkg.fuses["2"] === 48 && pkg.fuses["3"] === 48, JSON.stringify(pkg.fuses));

async function launch(label, extraEnv = {}) {
  const data = join(work, `data-${label}`);
  rmSync(data, { recursive: true, force: true });
  mkdirSync(join(work, `root-${label}`), { recursive: true });
  mkdirSync(data, { recursive: true });
  const p = spawn(pkg.bin, [], {
    stdio: "ignore",
    env: { ...process.env, KAKU_BIN: join(repo, "target/debug/kaku"), KAKUREGA_DATA_DIR: data, KAKU_ARGS: `--root ${join(work, `root-${label}`)} --scripted ${join(repo, "tests/e2e/script.json")} --ui-dir ${join(repo, "apps/web/dist")}`, ...extraEnv },
  });
  let exited = null;
  p.on("exit", (c, sig) => (exited = c ?? sig ?? "exited"));
  let served = false;
  for (let i = 0; i < 40 && !served && exited === null; i++) {
    await new Promise((r) => setTimeout(r, 500));
    if (existsSync(join(data, "admin.json"))) {
      try { const a = JSON.parse(readFileSync(join(data, "admin.json"), "utf8")); const r = await fetch(`http://127.0.0.1:${a.port}/`); served = r.status === 403 && (await r.text()).includes("SHELL_ONLY"); } catch {}
    }
  }
  const stillRunning = exited === null;
  p.kill("SIGKILL");
  spawnSync("pkill", ["-f", `kaku serve --data-dir ${data}`]);
  await new Promise((r) => setTimeout(r, 300));
  return { served, exited, stillRunning, data };
}

const clean = await launch("clean");
check("the packaged app starts from its asar and its companion serves the workspace", clean.served);

const asarPath = pkg.asarPath;
const backup = join(work, "app.asar.orig");
copyFileSync(asarPath, backup);
const flip = (offset) => {
  const fd = openSync(asarPath, "r+");
  const b = Buffer.alloc(1);
  readSync(fd, b, 0, 1, offset);
  b[0] ^= 0xff;
  writeSync(fd, b, 0, 1, offset);
  closeSync(fd);
};
flip(20);
const headerTampered = await launch("header");
check("an asar whose header was changed does not start", !headerTampered.served && headerTampered.exited !== null, `exit=${headerTampered.exited}`);
copyFileSync(backup, asarPath);
flip(statSync(asarPath).size - 40);
const bodyTampered = await launch("body");
check("an asar whose file contents were changed does not run the changed code", !bodyTampered.served, `exit=${bodyTampered.exited} served=${bodyTampered.served}`);
copyFileSync(backup, asarPath);

const marker = join(work, "evil-ran");
const loose = join(pkg.app, "Contents/Resources/app");
mkdirSync(loose, { recursive: true });
writeFileSync(join(loose, "package.json"), JSON.stringify({ name: "evil", main: "main.js" }));
writeFileSync(join(loose, "main.js"), `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'ran'); require('electron').app.quit();`);
const withLoose = await launch("loose");
check("a loose app folder planted beside the asar is ignored and never runs", !existsSync(marker) && withLoose.served, `marker=${existsSync(marker)} served=${withLoose.served}`);
rmSync(loose, { recursive: true, force: true });

const nodeEnv = await launch("nodeenv", { ELECTRON_RUN_AS_NODE: "1", NODE_OPTIONS: "--inspect=9238" });
check("with ELECTRON_RUN_AS_NODE and NODE_OPTIONS set the packaged binary still starts as the app, not as Node", nodeEnv.served);

const restored = await launch("restored");
check("after restoring the original asar the app starts again (the failures above were caused by the tampering)", restored.served);
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} packaged-app checks passed` : `\n${bad} FAILED`);
process.exit(bad ? 1 : 0);
