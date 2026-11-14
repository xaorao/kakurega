import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = join(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = JSON.parse(readFileSync(join(here, "manifest.json"), "utf8"));

test("ex06 the permission set is exactly the minimum and nothing broader is declared", () => {
  assert.equal(manifest.manifest_version, 3);
  assert.deepEqual([...manifest.permissions].sort(), ["contextMenus", "storage"]);
  assert.deepEqual(manifest.host_permissions, ["http://127.0.0.1/*"]);
  for (const banned of ["optional_permissions", "optional_host_permissions", "content_scripts", "externally_connectable", "web_accessible_resources", "nativeMessaging", "declarative_net_request", "devtools_page", "chrome_url_overrides", "oauth2", "key"]) {
    assert.equal(manifest[banned], undefined, `${banned} must not be declared`);
  }
  for (const p of ["tabs", "scripting", "activeTab", "cookies", "history", "bookmarks", "downloads", "webRequest", "identity", "clipboardRead", "management", "debugger", "<all_urls>"]) {
    assert.ok(!manifest.permissions.includes(p) && !manifest.host_permissions.includes(p), `${p} must not be requested`);
  }
});

test("ex07 the content security policy allows only the extension's own scripts", () => {
  const csp = manifest.content_security_policy.extension_pages;
  assert.match(csp, /script-src 'self'/);
  assert.match(csp, /object-src 'none'/);
  assert.ok(!/unsafe-inline|unsafe-eval|wasm-unsafe-eval/.test(csp), csp);
  // the only address named anywhere in the policy is this computer's, and only for requests: no page of the extension
  // can load code from, or talk to, any other place
  const parts = Object.fromEntries(csp.split(";").map((d) => d.trim().split(/\s+/)).map(([k, ...v]) => [k, v.join(" ")]));
  assert.equal(parts["connect-src"], "http://127.0.0.1:*");
  assert.equal(parts["default-src"], "'none'");
  for (const [k, v] of Object.entries(parts)) if (k !== "connect-src") assert.ok(!/https?:|\*/.test(v), `${k} ${v}`);
});

test("ex08 no source file loads remote code, evaluates strings or writes HTML from data", () => {
  const files = readdirSync(here).filter((f) => /\.(js|html|css)$/.test(f));
  assert.ok(files.length >= 6);
  for (const f of files) {
    const text = readFileSync(join(here, f), "utf8");
    assert.ok(!/\beval\s*\(|new Function\s*\(|innerHTML|outerHTML|insertAdjacentHTML|document\.write|importScripts/.test(text), `${f} uses a forbidden pattern`);
    const urls = text.match(/https?:\/\/[^\s"'`)]+/g) ?? [];
    for (const u of urls) assert.ok(u.startsWith("http://127.0.0.1"), `${f} mentions ${u}`);
  }
  for (const f of files.filter((f) => f.endsWith(".html"))) {
    const html = readFileSync(join(here, f), "utf8");
    assert.ok(!/<script(?![^>]*\bsrc=)[^>]*>/.test(html), `${f} has an inline script`);
    assert.ok(!/\son[a-z]+=/i.test(html), `${f} has an inline event handler`);
    assert.ok(!/<(iframe|object|embed)/i.test(html));
  }
});

test("ex09 every file the manifest names exists and the popup is wired to local scripts only", () => {
  assert.ok(existsSync(join(here, manifest.background.service_worker)));
  assert.ok(existsSync(join(here, manifest.action.default_popup)));
  const popup = readFileSync(join(here, "popup.html"), "utf8");
  assert.deepEqual([...popup.matchAll(/(?:src|href)="([^"]+)"/g)].map((m) => m[1]).sort(), ["popup.css", "popup.js"]);
});
