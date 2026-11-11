// Profiles, as the desktop shell sees them. The rules are the command line's (crates/kk-cli/src/profile.rs): the first
// profile is the data folder itself and is called `default`; a named one is `profiles/NAME` inside it and counts only when
// it holds the marker written at creation; the profile in use is named in the file `current-profile`. The shell only reads
// the list and writes that one file. Making and removing profiles stays with `kaku profile`, where removal asks for the name.
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export const DEFAULT = "default";
const NAME = /^[a-z0-9](?:[a-z0-9-]{0,30}[a-z0-9])?$/;

export const validName = (n) => typeof n === "string" && NAME.test(n);

export function profileDir(home, name) {
  if (name === DEFAULT) return home;
  return validName(name) ? join(home, "profiles", name) : null;
}

export function profileExists(home, name) {
  if (name === DEFAULT) return true;
  const dir = profileDir(home, name);
  if (!dir) return false;
  try {
    return lstatSync(dir).isDirectory() && existsSync(join(dir, "profile.json"));
  } catch {
    return false;
  }
}

/** The named profiles, sorted; `default` is not in the list. */
export function profileNames(home) {
  try {
    return readdirSync(join(home, "profiles")).filter((n) => validName(n) && profileExists(home, n)).sort();
  } catch {
    return [];
  }
}

/** The profile in use. A saved choice that is malformed or gone counts as `default`. */
export function currentProfile(home) {
  try {
    const n = readFileSync(join(home, "current-profile"), "utf8").trim();
    return n !== DEFAULT && profileExists(home, n) ? n : DEFAULT;
  } catch {
    return DEFAULT;
  }
}

/** Saves the choice, whole or not at all. False for a profile that does not exist. */
export function chooseProfile(home, name) {
  if (!profileExists(home, name)) return false;
  const file = join(home, "current-profile");
  if (name === DEFAULT) {
    rmSync(file, { force: true });
    return true;
  }
  mkdirSync(home, { recursive: true });
  writeFileSync(`${file}.tmp`, `${name}\n`, { mode: 0o600 });
  renameSync(`${file}.tmp`, file);
  return true;
}
