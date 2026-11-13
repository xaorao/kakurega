import { cpSync, mkdirSync, rmSync, writeFileSync, readFileSync, copyFileSync, chmodSync, existsSync, statSync, renameSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { flipFuses, getCurrentFuseWire, FuseVersion, FuseV1Options } from "@electron/fuses";
import { requireCurrentElectron } from "./electron-current.mjs";
import { MANIFEST, bundledPaths, checkPart, makeManifest, sha256File } from "../src/bundle.mjs";

const require = createRequire(import.meta.url);
const asar = require("@electron/asar");
const here = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(here, "..");
const repoRoot = resolve(appDir, "../..");

/** The app's name and macOS identity. The helpers' identities follow from it, as the Electron packager makes them. */
export const APP_NAME = "Kakurega";
export const APP_ID = "org.kakurega.desktop";
/**
 * What macOS shows when the calendar reader first asks. macOS asks on behalf of the app that started the reader, so the app
 * and every one of its helper processes carry the texts (without them the request is refused, or the reader is stopped).
 */
export const USAGE = {
  NSCalendarsFullAccessUsageDescription: "Kakurega reads your calendar on this computer only when you attach it to a question. Nothing is sent anywhere unless you send that question.",
  NSCalendarsUsageDescription: "Kakurega reads your calendar on this computer only when you attach it to a question. Nothing is sent anywhere unless you send that question.",
  NSRemindersFullAccessUsageDescription: "Kakurega reads your open reminders on this computer only when you attach them to a question. Nothing is sent anywhere unless you send that question.",
  NSRemindersUsageDescription: "Kakurega reads your open reminders on this computer only when you attach them to a question. Nothing is sent anywhere unless you send that question.",
};
/** Electron's helper apps in Contents/Frameworks, and whether a helper's identity leaves its suffix out (as the packager does). */
const HELPERS = [
  [" Helper", null],
  [" Helper (Renderer)", true],
  [" Helper (Plugin)", true],
  [" Helper (GPU)", true],
  [" Helper EH", false],
  [" Helper NP", false],
];

/**
 * Gives the copied Electron runtime its own name and identity: the app, its program and each helper app are renamed and their
 * Info.plist names, identities, version and usage texts set, the way @electron/packager (MIT) does it in src/mac.ts
 * (updatePlistFiles, moveHelpers). Without this the app calls itself Electron to macOS, and a permission given to it is a
 * permission given to "com.github.Electron".
 */
export function brand(appPath, version) {
  const plistOf = (app) => join(app, "Contents/Info.plist");
  // plutil takes the value as its own argument, so names and texts with spaces are written as they are
  const set = (plist, key, type, value) => execFileSync("/usr/bin/plutil", ["-replace", key, `-${type}`, value, plist], { stdio: "pipe" });
  const common = (plist, display, executable, id, name) => {
    set(plist, "CFBundleDisplayName", "string", display);
    set(plist, "CFBundleExecutable", "string", executable);
    set(plist, "CFBundleIdentifier", "string", id);
    set(plist, "CFBundleName", "string", name);
    set(plist, "CFBundleShortVersionString", "string", version);
    set(plist, "CFBundleVersion", "string", version);
    for (const [key, text] of Object.entries(USAGE)) set(plist, key, "string", text);
  };
  const frameworks = join(appPath, "Contents/Frameworks");
  for (const [suffix, ignoresSuffix] of HELPERS) {
    const from = join(frameworks, `Electron${suffix}.app`);
    if (!existsSync(from)) continue;
    const newName = `${APP_NAME}${suffix}`;
    const macos = join(from, "Contents/MacOS");
    renameSync(join(macos, `Electron${suffix}`), join(macos, newName));
    const to = join(frameworks, `${newName}.app`);
    renameSync(from, to);
    const tag = suffix.replace(/^ Helper ?/, "").replace(/[()]/g, "");
    const id = ignoresSuffix === false && tag ? `${APP_ID}.helper.${tag}` : `${APP_ID}.helper`;
    common(plistOf(to), newName, newName, id, ignoresSuffix === null ? APP_NAME : newName);
  }
  const macos = join(appPath, "Contents/MacOS");
  renameSync(join(macos, "Electron"), join(macos, APP_NAME));
  common(plistOf(appPath), APP_NAME, APP_NAME, APP_ID, APP_NAME);
  // the web app's mark (made by scripts/icon.swift), in place of Electron's
  copyFileSync(join(appDir, "assets/kakurega.icns"), join(appPath, "Contents/Resources/kakurega.icns"));
  rmSync(join(appPath, "Contents/Resources/electron.icns"), { force: true });
  set(plistOf(appPath), "CFBundleIconFile", "string", "kakurega.icns");
  return join(macos, APP_NAME);
}

/** The test build of the Secure Enclave helper answers its own dialog; it must never be shipped. Its build leaves this name in it. */
export const TEST_HELPER_MARK = "KAKU_TEST_DECISION";

// A packaged app is: the Electron runtime, our code in one asar whose header hash is written into Info.plist, fuses that make
// the runtime load only that asar, verify it, and refuse Node and inspector entry points, and the parts that live outside the
// asar (the kaku service and Secure Enclave helper in Contents/MacOS, the web interface in Contents/Resources/web) with their
// SHA-256 values in bundle.json inside the asar, which the shell checks before it runs them.
// cookieEncryption stays on for real builds; unattended tests turn it off because it makes macOS ask for the login
// keychain password, which blocks the run.
export async function packageApp(outDir, { cookieEncryption = true, kaku, web, enclave = null, ocr = null, calendar = null } = {}) {
  if (!kaku || !existsSync(kaku)) throw new Error(`the kaku binary to bundle was not found: ${kaku}`);
  if (!web || !existsSync(join(web, "index.html"))) throw new Error(`the web interface to bundle was not found (no index.html in ${web})`);
  if (enclave && readFileSync(enclave).includes(TEST_HELPER_MARK)) throw new Error(`${enclave} is the test build of the signing helper, which approves without asking; build the shipped one with spikes/enclave/build.sh`);
  const electronApp = join(appDir, "node_modules/electron/dist/Electron.app");
  const out = join(outDir, "Kakurega.app");
  rmSync(outDir, { recursive: true, force: true });
  mkdirSync(outDir, { recursive: true });
  cpSync(electronApp, out, { recursive: true, verbatimSymlinks: true });
  const bin = brand(out, JSON.parse(readFileSync(join(appDir, "package.json"), "utf8")).version);
  const parts = bundledPaths(join(out, "Contents/Resources"));
  // The signature of a nested program is part of its bytes, and signing the whole bundle at the end signs it again. Signing each
  // one here first, under its final name, gives the bytes that last step leaves (ad hoc signing of the same file is repeatable),
  // so the values recorded below are the values of what ships; that is checked again after the last signature.
  const signOne = (p) => execFileSync("codesign", ["--force", "--sign", "-", p], { stdio: "pipe" });
  copyFileSync(kaku, parts.kaku);
  chmodSync(parts.kaku, 0o755);
  signOne(parts.kaku);
  if (enclave) {
    copyFileSync(enclave, parts.enclave);
    chmodSync(parts.enclave, 0o755);
    signOne(parts.enclave);
  }
  // kaku's own macOS helpers go beside it, where it looks for them
  const helpers = { ocr, calendar };
  for (const [name, from] of Object.entries(helpers)) {
    if (!from) continue;
    if (!existsSync(from)) throw new Error(`the ${name} helper to bundle was not found: ${from}`);
    copyFileSync(from, parts[name]);
    chmodSync(parts[name], 0o755);
    signOne(parts[name]);
  }
  cpSync(web, parts.web, { recursive: true, verbatimSymlinks: true });
  const stage = join(outDir, "stage");
  mkdirSync(stage, { recursive: true });
  cpSync(join(appDir, "src"), join(stage, "src"), { recursive: true });
  copyFileSync(join(appDir, "package.json"), join(stage, "package.json"));
  const pkg = JSON.parse(readFileSync(join(stage, "package.json"), "utf8"));
  delete pkg.devDependencies;
  writeFileSync(join(stage, "package.json"), JSON.stringify(pkg, null, 2));
  // the values are taken from the copies inside the bundle, so they describe exactly what ships
  const manifest = makeManifest({ kaku: parts.kaku, web: parts.web, enclave: enclave ? parts.enclave : null, ocr: ocr ? parts.ocr : null, calendar: calendar ? parts.calendar : null });
  writeFileSync(join(stage, MANIFEST), JSON.stringify(manifest, null, 2));
  const asarPath = join(out, "Contents/Resources/app.asar");
  await asar.createPackage(stage, asarPath);
  rmSync(stage, { recursive: true, force: true });
  const { headerString } = asar.getRawHeader(asarPath);
  const hash = createHash("sha256").update(headerString).digest("hex");
  const plist = join(out, "Contents/Info.plist");
  const pb = (...a) => execFileSync("/usr/libexec/PlistBuddy", ["-c", a.join(" "), plist], { stdio: "pipe" });
  pb("Delete", ":ElectronAsarIntegrity:Resources/default_app.asar");
  pb("Add", ":ElectronAsarIntegrity:Resources/app.asar", "dict");
  pb("Add", ":ElectronAsarIntegrity:Resources/app.asar:algorithm", "string", "SHA256");
  pb("Add", ":ElectronAsarIntegrity:Resources/app.asar:hash", "string", hash);
  rmSync(join(out, "Contents/Resources/default_app.asar"), { force: true });
  await flipFuses(bin, {
    version: FuseVersion.V1,
    resetAdHocDarwinSignature: true,
    [FuseV1Options.RunAsNode]: false,
    [FuseV1Options.EnableCookieEncryption]: cookieEncryption,
    [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
    [FuseV1Options.EnableNodeCliInspectArguments]: false,
    [FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true,
    [FuseV1Options.OnlyLoadAppFromAsar]: true,
    [FuseV1Options.GrantFileProtocolExtraPrivileges]: false,
  });
  // ad hoc: a signature without an identity. It seals the bundle's files against accidental change, not against someone who
  // can rewrite the app and sign it again; a Developer ID signature and notarization are not done (no identity exists yet)
  execFileSync("codesign", ["--force", "--deep", "--sign", "-", out], { stdio: "pipe" });
  const shipped = [["kaku", parts.kaku], ["web", parts.web], ...(enclave ? [["enclave", parts.enclave]] : []), ...Object.entries(helpers).filter(([, f]) => f).map(([n]) => [n, parts[n]])];
  for (const [part, path] of shipped) {
    if (checkPart(manifest, part, path)) throw new Error(`packaging left ${part} different from the value recorded in ${MANIFEST}; the bundle would refuse to start`);
  }
  return { app: out, bin, asarPath, hash, manifest, parts, fuses: await getCurrentFuseWire(bin) };
}

/** The download: the app in a zip made with ditto (which keeps the framework links and signatures), and its SHA-256 in a file beside it. */
export function zipApp(appPath, outDir, { version, arch = process.arch } = {}) {
  const name = `Kakurega-${version}-macos-${arch}.zip`;
  const zip = join(outDir, name);
  rmSync(zip, { force: true });
  execFileSync("/usr/bin/ditto", ["-c", "-k", "--sequesterRsrc", "--keepParent", appPath, zip], { stdio: "pipe" });
  const sha256 = sha256File(zip);
  const shaFile = `${zip}.sha256`;
  writeFileSync(shaFile, `${sha256}  ${basename(zip)}\n`);
  return { zip, sha256, shaFile, bytes: statSync(zip).size };
}

function option(args, name, fallback) {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? resolve(args[i + 1]) : fallback;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  requireCurrentElectron();
  const args = process.argv.slice(2);
  const outDir = args[0] && !args[0].startsWith("--") ? resolve(args[0]) : join(repoRoot, "target/packaged");
  const kaku = option(args, "--kaku", join(repoRoot, "target/release/kaku"));
  const web = option(args, "--web", join(repoRoot, "apps/web/dist"));
  const enclave = option(args, "--enclave", [join(repoRoot, "target/release/kaku-enclave"), join(repoRoot, "target/debug/kaku-enclave")].find((p) => existsSync(p)) ?? null);
  if (!existsSync(kaku)) {
    console.error(`No kaku at ${kaku}. Build it with: cargo build --release -p kk-cli (or pass --kaku PATH).`);
    process.exit(1);
  }
  const ocr = option(args, "--ocr", null);
  const calendar = option(args, "--calendar", null);
  const r = await packageApp(outDir, { kaku, web, enclave, ocr, calendar });
  const { version } = JSON.parse(readFileSync(join(appDir, "package.json"), "utf8"));
  const z = zipApp(r.app, outDir, { version });
  console.log(JSON.stringify({ app: r.app, asar_hash: r.hash, bundle: r.manifest, fuses: r.fuses, zip: z.zip, sha256: z.sha256, sha256_file: z.shaFile }, null, 2));
  if (!enclave) console.error("Note: no Secure Enclave helper was bundled (build it with spikes/enclave/build.sh); approvals then need a software key (`kaku approver enroll`).");
  console.error("This app is signed ad hoc only: no Developer ID signature and no Apple notarization. Publish the SHA-256 value above with the zip.");
}
