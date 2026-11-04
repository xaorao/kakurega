import { spawnSync, spawn } from "node:child_process";
import { cpSync, rmSync, mkdirSync, existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import net from "node:net";
import { harden } from "../../apps/desktop/scripts/harden.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const src = join(repo, "apps/desktop/node_modules/electron/dist/Electron.app");
const work = join(repo, "target/electron-hardened");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
cpSync(src, join(work, "Electron.app"), { recursive: true, verbatimSymlinks: true });
const stock = join(src, "Contents/MacOS/Electron");
const hard = join(work, "Electron.app/Contents/MacOS/Electron");
const results = [];
const check = (n, ok, d = "") => (results.push(ok), console.log(`${ok ? "PASS" : "FAIL"}  ${n}${d ? "  " + d : ""}`));

const probeDir = join(repo, "tests/e2e/probe");
const asNode = (bin) => {
  const out = join(work, `probe-${Math.random().toString(36).slice(2)}.txt`);
  spawnSync(bin, [probeDir], { env: { ...process.env, ELECTRON_RUN_AS_NODE: "1", PROBE_OUT: out }, timeout: 20000, killSignal: "SIGKILL", stdio: "ignore" });
  return existsSync(out) ? readFileSync(out, "utf8") : "did-not-run";
};
const base = asNode(stock);
check("control: the stock Electron can be turned into a plain Node interpreter by an environment variable", base === "false", base);

const wire = await harden(hard, { cookieEncryption: false });
check("fuses flipped: RunAsNode off, NODE_OPTIONS off, --inspect off, file-protocol privileges off (cookie encryption is left off in this unattended test: it makes macOS ask for the keychain password)", wire && Object.keys(wire).length > 0, JSON.stringify(wire));

const h = asNode(hard);
check("hardened: ELECTRON_RUN_AS_NODE no longer gives a Node interpreter (the script runs as an Electron app instead)", h === "true", h);

const probe = (bin, args, env = {}) =>
  new Promise((res) => {
    const p = spawn(bin, args, { env: { ...process.env, ...env }, stdio: "ignore" });
    const t0 = Date.now();
    const tryPort = () => {
      const s = net.connect(9239, "127.0.0.1");
      s.on("connect", () => { s.destroy(); p.kill(); res(true); });
      s.on("error", () => { if (Date.now() - t0 > 6000) { p.kill(); res(false); } else setTimeout(tryPort, 250); });
    };
    setTimeout(tryPort, 500);
  });
const stockInspect = await probe(stock, ["--inspect=9239", join(repo, "apps/desktop/test")]);
const hardInspect = await probe(hard, ["--inspect=9239", join(repo, "apps/desktop/test")]);
const hardEnv = await probe(hard, [join(repo, "apps/desktop/test")], { NODE_OPTIONS: "--inspect=9239" });
check("control: the stock Electron opens a debugger port when started with --inspect", stockInspect === true);
check("hardened: --inspect opens no debugger port", hardInspect === false);
check("hardened: NODE_OPTIONS=--inspect opens no debugger port", hardEnv === false);
// the hardened binary still runs the app
const data = join(work, "data");
mkdirSync(join(work, "root"), { recursive: true });
mkdirSync(data, { recursive: true });
const app = spawn(hard, [join(repo, "apps/desktop")], { stdio: "ignore", env: { ...process.env, KAKU_BIN: join(repo, "target/debug/kaku"), KAKUREGA_DATA_DIR: data, KAKU_ARGS: `--root ${join(work, "root")} --scripted ${join(repo, "tests/e2e/script.json")} --ui-dir ${join(repo, "apps/web/dist")}` } });
let served = false;
for (let i = 0; i < 80 && !served; i++) {
  await new Promise((r) => setTimeout(r, 500));
  if (existsSync(join(data, "admin.json"))) {
    try { const a = JSON.parse(readFileSync(join(data, "admin.json"), "utf8")); served = (await fetch(`http://127.0.0.1:${a.port}/`)).status === 200; } catch {}
  }
}
spawnSync("pkill", ["-9", "-f", "electron-hardened"]);
spawnSync("pkill", ["-f", "target/debug/kaku serve --data-dir " + data]);
check("hardened: the app still starts and its companion serves the workspace", served);
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} fuse checks passed` : `\n${bad} FAILED`);
process.exit(bad ? 1 : 0);
