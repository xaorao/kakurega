import { cpSync, mkdirSync, rmSync, writeFileSync, readFileSync, copyFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { flipFuses, getCurrentFuseWire, FuseVersion, FuseV1Options } from "@electron/fuses";

const require = createRequire(import.meta.url);
const asar = require("@electron/asar");
const here = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(here, "..");

// A packaged app is: the Electron runtime, our code in one asar whose header hash is written into Info.plist, and
// fuses that make the runtime load only that asar, verify it, and refuse Node and inspector entry points.
export async function packageApp(outDir) {
  const electronApp = join(appDir, "node_modules/electron/dist/Electron.app");
  const out = join(outDir, "Kakurega.app");
  rmSync(outDir, { recursive: true, force: true });
  mkdirSync(outDir, { recursive: true });
  cpSync(electronApp, out, { recursive: true, verbatimSymlinks: true });
  const stage = join(outDir, "stage");
  mkdirSync(stage, { recursive: true });
  cpSync(join(appDir, "src"), join(stage, "src"), { recursive: true });
  copyFileSync(join(appDir, "package.json"), join(stage, "package.json"));
  const pkg = JSON.parse(readFileSync(join(stage, "package.json"), "utf8"));
  delete pkg.devDependencies;
  writeFileSync(join(stage, "package.json"), JSON.stringify(pkg, null, 2));
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
  const bin = join(out, "Contents/MacOS/Electron");
  await flipFuses(bin, {
    version: FuseVersion.V1,
    resetAdHocDarwinSignature: true,
    [FuseV1Options.RunAsNode]: false,
    [FuseV1Options.EnableCookieEncryption]: true,
    [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
    [FuseV1Options.EnableNodeCliInspectArguments]: false,
    [FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true,
    [FuseV1Options.OnlyLoadAppFromAsar]: true,
    [FuseV1Options.GrantFileProtocolExtraPrivileges]: false,
  });
  execFileSync("codesign", ["--force", "--deep", "--sign", "-", out], { stdio: "pipe" });
  return { app: out, bin, asarPath, hash, fuses: await getCurrentFuseWire(bin) };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const r = await packageApp(process.argv[2] ?? join(appDir, "../../target/packaged"));
  console.log(JSON.stringify({ app: r.app, hash: r.hash, fuses: r.fuses }));
}
