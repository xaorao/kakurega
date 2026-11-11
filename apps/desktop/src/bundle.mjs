import { createHash } from "node:crypto";
import { existsSync, lstatSync, readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";

// The parts of a packaged app that live outside its asar: the kaku service, the Secure Enclave helper and the web
// interface. Packaging writes their SHA-256 values into `bundle.json` inside the asar, which Electron's asar integrity check
// covers, and the shell compares the files with those values before it runs or serves any of them. A part that differs is
// not started. What this does not cover is written in docs/engineering-decisions/desktop-shell.md.

export const MANIFEST = "bundle.json";
const SHA = /^[0-9a-f]{64}$/;

export function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

/**
 * One value for a whole folder: every regular file's path (relative, with `/`) and SHA-256, in sorted order. A link or any
 * other kind of entry makes the folder unacceptable, so nothing can point the service outside it.
 */
export function sha256Tree(dir) {
  const lines = [];
  const walk = (d) => {
    for (const name of readdirSync(d).sort()) {
      const p = join(d, name);
      const st = lstatSync(p);
      if (st.isDirectory()) walk(p);
      else if (st.isFile()) lines.push(`${relative(dir, p).split(sep).join("/")}\0${sha256File(p)}\n`);
      else throw new Error(`${relative(dir, p)} is not a plain file`);
    }
  };
  walk(dir);
  return createHash("sha256").update(lines.join("")).digest("hex");
}

/** Where the outside parts sit in a macOS bundle, given `process.resourcesPath` (…/Kakurega.app/Contents/Resources). */
export function bundledPaths(resourcesPath) {
  return { kaku: join(resourcesPath, "..", "MacOS", "kaku"), enclave: join(resourcesPath, "..", "MacOS", "kaku-enclave"), web: join(resourcesPath, "web") };
}

/** The manifest packaging writes: the values of the parts it put in the bundle. The enclave helper is optional. */
export function makeManifest({ kaku, web, enclave = null }) {
  return { version: 1, kaku: sha256File(kaku), web: sha256Tree(web), ...(enclave ? { enclave: sha256File(enclave) } : {}) };
}

/** Reads the manifest from the app's own (integrity-checked) code folder. No manifest means a development or Linux build. */
export function readManifest(appPath) {
  const p = join(appPath, MANIFEST);
  if (!existsSync(p)) return null;
  const m = JSON.parse(readFileSync(p, "utf8"));
  if (m?.version !== 1 || !SHA.test(m.kaku ?? "") || !SHA.test(m.web ?? "") || (m.enclave !== undefined && !SHA.test(m.enclave))) throw new Error("bundle.json is damaged");
  return m;
}

/**
 * Checks one part against the manifest. Returns null when it matches, or the name of the part that does not: a missing
 * file, an unreadable one and a changed one all count as changed.
 */
export function checkPart(manifest, part, path) {
  try {
    const want = manifest[part];
    if (!SHA.test(want ?? "")) return part;
    const got = part === "web" ? sha256Tree(path) : sha256File(path);
    return got === want ? null : part;
  } catch {
    return part;
  }
}
