import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, existsSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chooseProfile, currentProfile, profileDir, profileExists, profileNames, validName } from "../src/profiles.mjs";
import { menuTemplate } from "../src/tray.mjs";
import { translator } from "../src/strings.mjs";

const make = (home, name) => {
  mkdirSync(join(home, "profiles", name), { recursive: true });
  writeFileSync(join(home, "profiles", name, "profile.json"), JSON.stringify({ name }));
};

test("pr01 only well-formed names are profiles, and a name never leaves the profiles folder", () => {
  const home = mkdtempSync(join(tmpdir(), "kk-prof-"));
  for (const bad of ["", "../x", "a/b", "Work", "a b", "-a", "a-", ".", "..", "a.b", "a".repeat(33), 7, null]) {
    assert.equal(validName(bad), false, String(bad));
    assert.equal(profileDir(home, bad), null);
    assert.equal(profileExists(home, bad), false);
  }
  assert.equal(profileDir(home, "default"), home);
  assert.equal(profileDir(home, "work"), join(home, "profiles", "work"));
});

test("pr02 a folder is a profile only with its marker; a link to one is not", () => {
  const home = mkdtempSync(join(tmpdir(), "kk-prof-"));
  assert.deepEqual(profileNames(home), []);
  make(home, "work");
  make(home, "alpha");
  mkdirSync(join(home, "profiles", "ghost"));
  symlinkSync(join(home, "profiles", "work"), join(home, "profiles", "linked"));
  assert.deepEqual(profileNames(home), ["alpha", "work"]);
});

test("pr03 the choice is saved whole, default removes it, and a choice that is gone or malformed counts as default", () => {
  const home = mkdtempSync(join(tmpdir(), "kk-prof-"));
  make(home, "work");
  assert.equal(currentProfile(home), "default");
  assert.equal(chooseProfile(home, "nope"), false);
  assert.equal(chooseProfile(home, "../../etc"), false);
  assert.equal(chooseProfile(home, "work"), true);
  assert.equal(readFileSync(join(home, "current-profile"), "utf8"), "work\n");
  assert.equal(currentProfile(home), "work");
  for (const junk of ["gone\n", "../../etc\n", "", "Work"]) {
    writeFileSync(join(home, "current-profile"), junk);
    assert.equal(currentProfile(home), "default", JSON.stringify(junk));
  }
  chooseProfile(home, "work");
  assert.equal(chooseProfile(home, "default"), true);
  assert.equal(existsSync(join(home, "current-profile")), false);
});

test("pr04 the menu offers profiles only when there is more than one, marks the one in use, and reports the one chosen", () => {
  const t = translator("en");
  const chosen = [];
  const actions = { open() {}, inbox() {}, setNotify() {}, quit() {}, profile: (n) => chosen.push(n) };
  const one = menuTemplate({ count: 0, notify: true, profile: "default", profiles: ["default"] }, t, actions);
  assert.equal(one.find((i) => i.id === "profile"), undefined);
  const two = menuTemplate({ count: 0, notify: true, profile: "work", profiles: ["default", "work"] }, t, actions).find((i) => i.id === "profile");
  assert.equal(two.label, "Profile: work");
  assert.deepEqual(two.submenu.map((i) => [i.label, i.checked]), [["default", false], ["work", true]]);
  two.submenu[0].click();
  assert.deepEqual(chosen, ["default"]);
  assert.ok(menuTemplate({ count: 0, notify: true, profile: "work", profiles: ["default", "work"] }, translator("hi"), actions).find((i) => i.id === "profile").label.includes("work"));
});
