import { spawn, spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, openSync, readFileSync, readSync, readdirSync, rmSync, statSync, symlinkSync, writeFileSync, writeSync, closeSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { packageApp, zipApp } from "../../apps/desktop/scripts/package.mjs";

// The published download as a stranger meets it: the zip and its SHA-256, installed with scripts/install-desktop-macos into a
// folder whose name has spaces, then opened with nothing from outside the app (no KAKU_BIN). The hostile cases are a zip that
// was changed after its value was published, and zips that match their own value but are not a single, intact Kakurega.app.
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/desktop-install");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
const installer = join(repo, "scripts/install-desktop-macos");
const tmp = join(work, "tmp");
mkdirSync(tmp, { recursive: true });
const results = [];
const check = (n, ok, d = "") => (results.push(ok), console.log(`${ok ? "PASS" : "FAIL"}  ${n}${d ? "  " + d : ""}`));
const sha = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
const install = (...args) => {
  const r = spawnSync(installer, args, { encoding: "utf8", env: { ...process.env, TMPDIR: tmp }, timeout: 300000 });
  return { code: r.status, out: `${r.stdout}${r.stderr}` };
};
const leftovers = () => readdirSync(tmp).filter((n) => n.startsWith("kakurega-install."));
const marker = join(work, "archive-code-ran");
const zipOf = (dir, name) => {
  const z = join(work, name);
  spawnSync("/usr/bin/ditto", ["-c", "-k", "--keepParent", dir, z]);
  return z;
};

const enclave = join(repo, "target/debug/kaku-enclave");
const pkg = await packageApp(join(work, "pkg"), { cookieEncryption: false, kaku: join(repo, "target/debug/kaku"), web: join(repo, "apps/web/dist"), enclave: existsSync(enclave) ? enclave : null });
const { version } = JSON.parse(readFileSync(join(repo, "apps/desktop/package.json"), "utf8"));
const z = zipApp(pkg.app, join(work, "pkg"), { version });
check("packaging makes a zip and a .sha256 file whose value is the zip's", existsSync(z.zip) && readFileSync(z.shaFile, "utf8") === `${sha(z.zip)}  ${z.zip.split("/").pop()}\n`, `${(z.bytes / 1e6).toFixed(0)} MB`);
const downloads = join(work, "My Downloads");
const dest = join(work, "My Applications");
mkdirSync(downloads, { recursive: true });
const zip = join(downloads, "Kakurega download.zip");
copyFileSync(z.zip, zip);
const shaFile = join(downloads, "Kakurega download.zip.sha256");
copyFileSync(z.shaFile, shaFile);

// a zip changed after its value was published: refused before anything is unpacked
const tampered = join(downloads, "Kakurega tampered.zip");
copyFileSync(zip, tampered);
const fd = openSync(tampered, "r+");
const b = Buffer.alloc(1);
const at = Math.floor(statSync(tampered).size / 2);
readSync(fd, b, 0, 1, at);
b[0] ^= 0xff;
writeSync(fd, b, 0, 1, at);
closeSync(fd);
const t1 = install(tampered, z.sha256, dest);
check("hostile: a zip changed by one byte is refused, both values are shown, and nothing is unpacked or installed", t1.code === 1 && /does not match the published SHA-256/.test(t1.out) && t1.out.includes(z.sha256) && !existsSync(join(dest, "Kakurega.app")) && leftovers().length === 0, t1.out.split("\n")[0]);
for (const badValue of ["", "abc", "g".repeat(64), z.sha256.slice(0, 63), `${z.sha256}0`]) {
  const r = install(zip, badValue, dest);
  if (r.code !== 1 || existsSync(join(dest, "Kakurega.app"))) check(`a published value that is not 64 hex characters is refused (${JSON.stringify(badValue.slice(0, 8))})`, false, r.out);
}
check("a published value that is not a SHA-256 is refused", !existsSync(join(dest, "Kakurega.app")));

// zips that match their own value but are not one intact Kakurega.app; none of their code may run
const evil = (name, build) => {
  const d = join(work, `evil-${name}`);
  const a = join(d, "Kakurega.app");
  mkdirSync(join(a, "Contents/MacOS"), { recursive: true });
  writeFileSync(join(a, "Contents/Info.plist"), `<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>Electron</string></dict></plist>`);
  writeFileSync(join(a, "Contents/MacOS/Electron"), `#!/bin/sh\ntouch '${marker}'\n`);
  chmodSync(join(a, "Contents/MacOS/Electron"), 0o755);
  build?.(d, a);
  return zipOf(a, `evil-${name}.zip`);
};
const unsigned = evil("unsigned");
const r1 = install(unsigned, sha(unsigned), dest);
check("hostile: a zip with an app whose signature is not intact is refused, and nothing inside it ran", r1.code === 1 && /signature is broken/.test(r1.out) && !existsSync(marker) && !existsSync(join(dest, "Kakurega.app")), r1.out.split("\n").find((l) => l.startsWith("REFUSED")));
const linked = evil("link", (_d, a) => symlinkSync("/etc", join(a, "Contents/Resources")));
const r2 = install(linked, sha(linked), dest);
check("hostile: an app with a link pointing outside itself is refused", r2.code === 1 && /link in the app points outside it/.test(r2.out) && !existsSync(join(dest, "Kakurega.app")), r2.out.split("\n").find((l) => l.startsWith("REFUSED")));
const two = join(work, "evil-two");
mkdirSync(join(two, "Kakurega.app/Contents"), { recursive: true });
writeFileSync(join(two, "run-me.command"), `#!/bin/sh\ntouch '${marker}'\n`);
chmodSync(join(two, "run-me.command"), 0o755);
const twoZip = join(work, "evil-two.zip");
spawnSync("/usr/bin/ditto", ["-c", "-k", two, twoZip]);
const r3 = install(twoZip, sha(twoZip), dest);
check("hostile: a zip holding anything beside Kakurega.app is refused, and the extra file never ran", r3.code === 1 && /exactly Kakurega.app/.test(r3.out) && !existsSync(marker), r3.out.split("\n").find((l) => l.startsWith("REFUSED")));
check("no temporary folder is left behind by any refusal", leftovers().length === 0, leftovers().join(" "));

// the real download, with the value read from its .sha256 file, into a folder with spaces in its name
const ok = install(zip, shaFile, dest);
const installed = join(dest, "Kakurega.app");
check("the published zip installs into a folder whose name has spaces", ok.code === 0 && existsSync(join(installed, "Contents/MacOS/kaku")) && existsSync(join(installed, "Contents/Resources/web/index.html")) && spawnSync("codesign", ["--verify", "--deep", "--strict", installed]).status === 0, ok.out.split("\n").slice(0, 2).join(" | "));
check("the installer says plainly that the app is signed ad hoc only and not notarized", /signed ad hoc only/.test(ok.out) && /not notarized by Apple/.test(ok.out));
writeFileSync(join(installed, "Contents/Resources/left-over-from-before"), "x");
const again = install(zip, z.sha256.toUpperCase(), dest);
check("installing again replaces the installed app as a whole (a value in capitals is accepted)", again.code === 0 && !existsSync(join(installed, "Contents/Resources/left-over-from-before")) && readdirSync(dest).length === 1, readdirSync(dest).join(","));

// the installed app opens with only what it carries
const data = join(work, "data");
const root = join(work, "root");
async function openInstalled() {
  for (let attempt = 0; attempt < 6; attempt++) {
    rmSync(data, { recursive: true, force: true });
    mkdirSync(data, { recursive: true });
    mkdirSync(root, { recursive: true });
    const env = { ...process.env, KAKUREGA_DATA_DIR: data, KAKU_ARGS: `--root ${root} --scripted ${join(repo, "tests/e2e/script.json")}` };
    delete env.KAKU_BIN;
    const p = spawn(join(installed, "Contents/MacOS/Electron"), [], { stdio: "ignore", env });
    let served = false;
    for (let i = 0; i < 30 && !served; i++) {
      await new Promise((r) => setTimeout(r, 500));
      if (existsSync(join(data, "admin.json"))) {
        try { const a = JSON.parse(readFileSync(join(data, "admin.json"), "utf8")); const r = await fetch(`http://127.0.0.1:${a.port}/`); served = r.status === 403 && (await r.text()).includes("SHELL_ONLY"); } catch {}
      }
    }
    if (served) return p;
    p.kill("SIGKILL");
    spawnSync("pkill", ["-9", "-f", `serve --data-dir ${data}`]);
  }
  return null;
}
const running = await openInstalled();
const line = spawnSync("ps", ["-axo", "command"], { encoding: "utf8" }).stdout.split("\n").find((l) => l.includes(`serve --data-dir ${data}`)) ?? "";
check("the installed app starts its own bundled companion and web interface, with nothing from outside", !!running && line.startsWith(join(installed, "Contents/MacOS/kaku")) && line.includes(`--ui-dir ${join(installed, "Contents/Resources/web")}`), line.slice(0, 200));
const busy = install(zip, z.sha256, dest);
check("the installer will not replace an app that is running", busy.code === 1 && /quit it first/.test(busy.out) && existsSync(installed), busy.out.split("\n").find((l) => l.startsWith("REFUSED")));
running?.kill("SIGKILL");
spawnSync("pkill", ["-9", "-f", `serve --data-dir ${data}`]);
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} install checks passed` : `\n${bad} FAILED`);
process.exit(bad ? 1 : 0);
