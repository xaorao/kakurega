import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { electronCheck, requireCurrentElectron } from "../scripts/electron-current.mjs";

const here = join(dirname(fileURLToPath(import.meta.url)), "..");
const asked = [];
const fake = (status, stdout = "", stderr = "", error = undefined) => (cmd, args) => {
  asked.push([cmd, args]);
  return { status, stdout, stderr, error };
};

test("ec01 a current Electron lets packaging go on, and the check is asked in strict mode", () => {
  asked.length = 0;
  const r = electronCheck({ run: fake(0, "electron 45.2.3: current\n"), env: {}, root: "/r" });
  assert.deepEqual([r.ok, r.skipped, r.message], [true, false, "electron 45.2.3: current"]);
  assert.equal(asked[0][0], "python3");
  assert.deepEqual(asked[0][1], ["/r/scripts/check_electron.py", "--strict"]);
});

test("ec02 a pin that is behind, a line that is unsupported and a registry that could not be asked each stop packaging, in the checker's words", () => {
  for (const said of ["electron 45.2.2: behind (newest of its line 45.2.3)", "electron 43.0.0: unsupported", "electron 45.2.3: could not ask the registry (URLError); not checked"]) {
    const r = electronCheck({ run: fake(1, said + "\n"), env: {}, root: "/r" });
    assert.deepEqual([r.ok, r.message], [false, said]);
  }
});

test("ec03 a checker that cannot even start stops packaging and says why", () => {
  const r = electronCheck({ run: fake(null, "", "", { code: "ENOENT" }), env: {}, root: "/r" });
  assert.equal(r.ok, false);
  assert.match(r.message, /could not run \(ENOENT\)/);
});

test("ec04 the offline override is exactly KAKU_SKIP_ELECTRON_CHECK=1, is said out loud, and asks nobody", () => {
  asked.length = 0;
  const r = electronCheck({ run: fake(1, "never asked"), env: { KAKU_SKIP_ELECTRON_CHECK: "1" }, root: "/r" });
  assert.deepEqual([r.ok, r.skipped], [true, true]);
  assert.match(r.message, /not checked/);
  assert.equal(asked.length, 0);
  for (const other of ["0", "true", "yes", "", " 1"]) {
    assert.equal(electronCheck({ run: fake(1, "behind"), env: { KAKU_SKIP_ELECTRON_CHECK: other }, root: "/r" }).ok, false, JSON.stringify(other));
  }
});

test("ec05 both packaging commands ask before they package, and the functions the tests call do not", () => {
  for (const file of ["package.mjs", "package-linux.mjs"]) {
    const src = readFileSync(join(here, "scripts", file), "utf8");
    const entry = src.split("if (import.meta.url === `file://${process.argv[1]}`) {")[1];
    assert.ok(entry && entry.trimStart().startsWith("requireCurrentElectron();"), `${file} asks first in its command entry`);
    const before = src.split("if (import.meta.url === `file://${process.argv[1]}`) {")[0];
    assert.ok(!/requireCurrentElectron\(\)/.test(before.replace(/import \{ requireCurrentElectron \}[^\n]*\n/, "")), `${file} does not ask inside the exported function`);
  }
});

test("ec06 a failed check ends the command with exit status 1 and tells the person what to do; a good one does not", () => {
  const said = [];
  const exits = [];
  requireCurrentElectron({ run: fake(1, "electron 45.2.2: behind"), env: {}, root: "/r", exit: (c) => exits.push(c), say: (m) => said.push(m) });
  assert.deepEqual(exits, [1]);
  assert.ok(said[0].includes("behind") && /Packaging stopped/.test(said[1]) && /KAKU_SKIP_ELECTRON_CHECK=1/.test(said[1]) && /apps\/desktop\/package\.json/.test(said[1]));
  exits.length = 0;
  requireCurrentElectron({ run: fake(0, "electron 45.2.3: current"), env: {}, root: "/r", exit: (c) => exits.push(c), say: () => {} });
  assert.deepEqual(exits, []);
});
