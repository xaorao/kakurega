// Builds the Linux package: a .deb holding the Electron runtime with our code in one asar and the fuses set, the kaku
// service and its sandboxed worker, and the web interface. Run on Linux (in the Lima VM from scripts/package-linux).
import { cpSync, mkdirSync, rmSync, writeFileSync, readFileSync, copyFileSync, chmodSync, renameSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { flipFuses, getCurrentFuseWire, FuseVersion, FuseV1Options } from "@electron/fuses";

const require = createRequire(import.meta.url);
const asar = require("@electron/asar");
const here = dirname(fileURLToPath(import.meta.url));
const appDir = resolve(here, "..");

export const INSTALL_DIR = "opt/kakurega";

// Embedded asar integrity validation is a macOS and Windows feature of Electron, so it is not claimed here: on Linux the
// protection is OnlyLoadAppFromAsar (a loose app folder is ignored), the closed Node and inspector doors, and the package
// files being owned by root.
export const LINUX_FUSES = {
  version: FuseVersion.V1,
  [FuseV1Options.RunAsNode]: false,
  [FuseV1Options.EnableCookieEncryption]: true,
  [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
  [FuseV1Options.EnableNodeCliInspectArguments]: false,
  [FuseV1Options.OnlyLoadAppFromAsar]: true,
  [FuseV1Options.GrantFileProtocolExtraPrivileges]: false,
};

export function launcherScript() {
  return `#!/bin/sh
# Starts the Kakurega desktop window. The window starts the local service itself and stops it when it closes.
export KAKU_BIN=/${INSTALL_DIR}/kaku
export KAKU_ARGS="--ui-dir /${INSTALL_DIR}/web"
exec /${INSTALL_DIR}/kakurega "$@"
`;
}

export function controlFile({ version, arch }) {
  return `Package: kakurega
Version: ${version}
Section: utils
Priority: optional
Architecture: ${arch}
Depends: bubblewrap, libgtk-3-0, libnss3, libxss1, libasound2 | libasound2t64, libgbm1
Maintainer: Kakurega
Description: Confidential AI workspace
 An AI works on your private files and code, a security layer outside the model decides what may leave, and the approved
 result reaches a recipient as a capsule they can check themselves. The local service, its sandboxed worker and the
 desktop window are installed under /${INSTALL_DIR}.
`;
}

export function desktopEntry() {
  return `[Desktop Entry]
Type=Application
Name=Kakurega
Comment=Confidential AI workspace
Exec=/usr/bin/kakurega
Terminal=false
Categories=Utility;Security;
`;
}

export const POSTINST = `#!/bin/sh
set -e
# Electron's sandbox helper must be owned by root and setuid so the renderer runs in a real sandbox
chown root:root /${INSTALL_DIR}/chrome-sandbox
chmod 4755 /${INSTALL_DIR}/chrome-sandbox
exit 0
`;

export async function packageLinux({ electronDist, kaku, worker, web, outDir, version }) {
  const stage = join(outDir, "stage");
  rmSync(outDir, { recursive: true, force: true });
  const root = join(stage, INSTALL_DIR);
  mkdirSync(root, { recursive: true });
  cpSync(electronDist, root, { recursive: true, verbatimSymlinks: true });
  rmSync(join(root, "resources/default_app.asar"), { force: true });
  const bundle = join(outDir, "bundle");
  mkdirSync(bundle, { recursive: true });
  cpSync(join(appDir, "src"), join(bundle, "src"), { recursive: true });
  const pkg = JSON.parse(readFileSync(join(appDir, "package.json"), "utf8"));
  delete pkg.devDependencies;
  writeFileSync(join(bundle, "package.json"), JSON.stringify(pkg, null, 2));
  await asar.createPackage(bundle, join(root, "resources/app.asar"));
  rmSync(bundle, { recursive: true, force: true });
  renameSync(join(root, "electron"), join(root, "kakurega"));
  await flipFuses(join(root, "kakurega"), LINUX_FUSES);
  copyFileSync(kaku, join(root, "kaku"));
  copyFileSync(worker, join(root, "kk-worker"));
  chmodSync(join(root, "kaku"), 0o755);
  chmodSync(join(root, "kk-worker"), 0o755);
  cpSync(web, join(root, "web"), { recursive: true });
  mkdirSync(join(stage, "usr/bin"), { recursive: true });
  writeFileSync(join(stage, "usr/bin/kakurega"), launcherScript(), { mode: 0o755 });
  mkdirSync(join(stage, "usr/share/applications"), { recursive: true });
  writeFileSync(join(stage, "usr/share/applications/kakurega.desktop"), desktopEntry());
  mkdirSync(join(stage, "DEBIAN"), { recursive: true });
  const arch = execFileSync("dpkg", ["--print-architecture"], { encoding: "utf8" }).trim();
  writeFileSync(join(stage, "DEBIAN/control"), controlFile({ version, arch }));
  writeFileSync(join(stage, "DEBIAN/postinst"), POSTINST, { mode: 0o755 });
  const deb = join(outDir, `kakurega_${version}_${arch}.deb`);
  execFileSync("dpkg-deb", ["--root-owner-group", "-Zxz", "-b", stage, deb], { stdio: "pipe" });
  return { deb, fuses: await getCurrentFuseWire(join(root, "kakurega")), installed: existsSync(root) };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [electronDist, kaku, worker, web, outDir, version] = process.argv.slice(2);
  const r = await packageLinux({ electronDist, kaku, worker, web, outDir, version });
  console.log(JSON.stringify({ deb: r.deb, fuses: r.fuses }));
}
