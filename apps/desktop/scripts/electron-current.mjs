import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../..");

/**
 * Asks `scripts/check_electron.py --strict` whether the pinned Electron is the newest release of a supported line. A pin that is
 * behind, a line that is no longer supported, and a registry that could not be asked all say no, because a package built on an
 * old Electron may be missing a security fix. Setting KAKU_SKIP_ELECTRON_CHECK=1 skips it, for a build made offline on purpose;
 * the skip is said out loud.
 */
export function electronCheck({ run = spawnSync, env = process.env, root = repoRoot } = {}) {
  if (env.KAKU_SKIP_ELECTRON_CHECK === "1") {
    return { ok: true, skipped: true, message: "Electron currency not checked: KAKU_SKIP_ELECTRON_CHECK=1 was set." };
  }
  const r = run("python3", [join(root, "scripts/check_electron.py"), "--strict"], { encoding: "utf8", timeout: 60000 });
  const said = `${r.stdout ?? ""}${r.stderr ?? ""}`.trim();
  if (r.status === 0) return { ok: true, skipped: false, message: said };
  return { ok: false, skipped: false, message: said || `the Electron check could not run (${r.error?.code ?? "no exit status"})` };
}

/** For the packaging commands: stops the process with a plain message unless the Electron is current. */
export function requireCurrentElectron({ exit = process.exit, say = console.error, ...options } = {}) {
  const r = electronCheck(options);
  say(r.message);
  if (!r.ok) {
    say(
      "Packaging stopped. Move the pin in apps/desktop/package.json to the newest release of its line and run the desktop suites, or set KAKU_SKIP_ELECTRON_CHECK=1 to package anyway (for a build made offline on purpose).",
    );
    exit(1);
  }
}
