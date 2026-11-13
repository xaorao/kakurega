import test from "node:test";
import assert from "node:assert/strict";
import { appendFileSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync, renameSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { bundledPaths, checkPart, makeManifest, readManifest, sha256Tree, MANIFEST } from "../src/bundle.mjs";
import { TEST_HELPER_MARK, packageApp } from "../scripts/package.mjs";

function fixture() {
  const d = mkdtempSync(join(tmpdir(), "kk-bundle-"));
  writeFileSync(join(d, "kaku"), "the service");
  writeFileSync(join(d, "kaku-enclave"), "the helper");
  mkdirSync(join(d, "web/assets"), { recursive: true });
  writeFileSync(join(d, "web/index.html"), "<!doctype html>");
  writeFileSync(join(d, "web/assets/app.js"), "console.log(1)");
  return d;
}

test("bu01 the manifest records every part, and an untouched bundle passes", () => {
  const d = fixture();
  const m = makeManifest({ kaku: join(d, "kaku"), web: join(d, "web"), enclave: join(d, "kaku-enclave") });
  assert.match(m.kaku, /^[0-9a-f]{64}$/);
  assert.equal(checkPart(m, "kaku", join(d, "kaku")), null);
  assert.equal(checkPart(m, "web", join(d, "web")), null);
  assert.equal(checkPart(m, "enclave", join(d, "kaku-enclave")), null);
  assert.equal(makeManifest({ kaku: join(d, "kaku"), web: join(d, "web") }).enclave, undefined, "the helper is optional");
  rmSync(d, { recursive: true });
});

test("bu02 hostile: one changed byte, an added, removed or renamed web file, a link, or a missing binary is caught", () => {
  const d = fixture();
  const m = makeManifest({ kaku: join(d, "kaku"), web: join(d, "web"), enclave: join(d, "kaku-enclave") });
  appendFileSync(join(d, "kaku"), "x");
  assert.equal(checkPart(m, "kaku", join(d, "kaku")), "kaku");
  rmSync(join(d, "kaku"));
  assert.equal(checkPart(m, "kaku", join(d, "kaku")), "kaku", "a missing binary counts as changed");
  const webBefore = sha256Tree(join(d, "web"));
  writeFileSync(join(d, "web/evil.js"), "steal()");
  assert.equal(checkPart(m, "web", join(d, "web")), "web");
  rmSync(join(d, "web/evil.js"));
  assert.equal(sha256Tree(join(d, "web")), webBefore);
  renameSync(join(d, "web/assets/app.js"), join(d, "web/assets/app2.js"));
  assert.equal(checkPart(m, "web", join(d, "web")), "web", "a renamed file changes the value");
  renameSync(join(d, "web/assets/app2.js"), join(d, "web/assets/app.js"));
  symlinkSync("/etc/passwd", join(d, "web/passwd"));
  assert.equal(checkPart(m, "web", join(d, "web")), "web", "a link is refused");
  rmSync(join(d, "web/passwd"));
  assert.equal(checkPart(m, "web", join(d, "web")), null);
  assert.equal(checkPart({ ...m, enclave: undefined }, "enclave", join(d, "kaku-enclave")), "enclave", "a part the manifest does not list is never accepted");
  rmSync(d, { recursive: true });
});

test("bu03 the manifest is read from the app folder; none means a development build, a damaged one is an error", () => {
  const d = fixture();
  assert.equal(readManifest(d), null);
  const m = makeManifest({ kaku: join(d, "kaku"), web: join(d, "web") });
  writeFileSync(join(d, MANIFEST), JSON.stringify(m));
  assert.deepEqual(readManifest(d), m);
  for (const bad of ["{", "{}", JSON.stringify({ ...m, version: 2 }), JSON.stringify({ ...m, kaku: "abc" }), JSON.stringify({ ...m, enclave: "zz" }), JSON.stringify({ ...m, ocr: "zz" }), JSON.stringify({ ...m, calendar: 7 })]) {
    writeFileSync(join(d, MANIFEST), bad);
    assert.throws(() => readManifest(d), undefined, bad);
  }
  rmSync(d, { recursive: true });
});

test("bu04 the parts sit beside the app's program and in its resources", () => {
  assert.deepEqual(bundledPaths("/A/Kakurega.app/Contents/Resources"), {
    kaku: "/A/Kakurega.app/Contents/MacOS/kaku",
    enclave: "/A/Kakurega.app/Contents/MacOS/kaku-enclave",
    ocr: "/A/Kakurega.app/Contents/MacOS/kaku-ocr",
    calendar: "/A/Kakurega.app/Contents/MacOS/kaku-calendar",
    web: "/A/Kakurega.app/Contents/Resources/web",
  });
});

test("bu04b kaku's helpers are recorded when bundled and absent from the manifest otherwise", () => {
  const d = fixture();
  writeFileSync(join(d, "kaku-ocr"), "ocr helper");
  writeFileSync(join(d, "kaku-calendar"), "calendar helper");
  const m = makeManifest({ kaku: join(d, "kaku"), web: join(d, "web"), ocr: join(d, "kaku-ocr"), calendar: join(d, "kaku-calendar") });
  assert.match(m.ocr, /^[0-9a-f]{64}$/);
  assert.match(m.calendar, /^[0-9a-f]{64}$/);
  assert.equal(checkPart(m, "calendar", join(d, "kaku-calendar")), null);
  writeFileSync(join(d, "kaku-calendar"), "changed");
  assert.equal(checkPart(m, "calendar", join(d, "kaku-calendar")), "calendar");
  const bare = makeManifest({ kaku: join(d, "kaku"), web: join(d, "web") });
  assert.equal(bare.ocr, undefined);
  assert.equal(bare.calendar, undefined);
  rmSync(d, { recursive: true });
});

test("bu05 packaging refuses the test build of the signing helper and a missing service or web interface, before building anything", async () => {
  const d = fixture();
  writeFileSync(join(d, "kaku-enclave-test"), `binary ${TEST_HELPER_MARK} more`);
  const out = join(d, "out");
  await assert.rejects(packageApp(out, { kaku: join(d, "kaku"), web: join(d, "web"), enclave: join(d, "kaku-enclave-test") }), /test build/);
  await assert.rejects(packageApp(out, { kaku: join(d, "nope"), web: join(d, "web") }), /kaku binary/);
  await assert.rejects(packageApp(out, { kaku: join(d, "kaku"), web: join(d, "assets") }), /web interface/);
  await assert.rejects(packageApp(out, {}), /kaku binary/);
  rmSync(d, { recursive: true });
});
