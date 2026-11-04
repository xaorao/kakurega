import { flipFuses, getCurrentFuseWire, FuseVersion, FuseV1Options } from "@electron/fuses";

export const WANTED = {
  [FuseV1Options.RunAsNode]: false,
  [FuseV1Options.EnableCookieEncryption]: true,
  [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
  [FuseV1Options.EnableNodeCliInspectArguments]: false,
  [FuseV1Options.GrantFileProtocolExtraPrivileges]: false,
};

export async function harden(electronBinary) {
  await flipFuses(electronBinary, { version: FuseVersion.V1, resetAdHocDarwinSignature: true, ...WANTED });
  return getCurrentFuseWire(electronBinary);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const bin = process.argv[2];
  if (!bin) {
    console.error("usage: node harden.mjs PATH/TO/Electron.app/Contents/MacOS/Electron");
    process.exit(2);
  }
  console.log(JSON.stringify(await harden(bin)));
}
