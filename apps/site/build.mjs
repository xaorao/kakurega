// Puts the public site together in apps/site/dist: the landing page, the one-file check page as /verify/, and a real
// capsule to try it on. Every fact the page states about that capsule (its model, the model's key, how many answers
// are signed, its size, how long it is valid, its findings and files) is read from the capsule here, so the page cannot
// say something the file does not. Nothing needs a server: every file is static.
//   node apps/site/build.mjs
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const out = join(here, "dist");
const capsulePath = join(repo, "fixtures/near-live/sample.kkg");
if (!existsSync(capsulePath)) throw new Error("the sample capsule is missing: " + capsulePath);

const built = spawnSync("node", ["build.mjs"], { cwd: join(repo, "apps/verifier-page"), encoding: "utf8" });
if (built.status !== 0) throw new Error("the check page did not build: " + built.stderr);

// the facts, from the capsule itself
const capsuleBytes = readFileSync(capsulePath);
const capsule = JSON.parse(capsuleBytes.toString("utf8"));
const b64 = (s) => Buffer.from(s.replace(/-/g, "+").replace(/_/g, "/"), "base64");
const release = JSON.parse(b64(capsule.envelopes[0].payload).toString("utf8"));
const receipts = release.assurance?.route_receipts ?? [];
if (receipts.length === 0) throw new Error("the sample capsule carries no model signatures; the page promises some");
const signers = new Set(receipts.map((r) => r.signer));
const models = new Set(receipts.map((r) => String(r.signed_text).split(":").slice(0, -2).join(":")));
if (signers.size !== 1 || models.size !== 1) throw new Error("the page names one model and one key; the capsule has " + models.size + " and " + signers.size);
const daysLeft = (release.expires - Date.now() / 1000) / 86400;
if (daysLeft < 120) throw new Error(`the sample capsule is valid for ${daysLeft.toFixed(1)} more days; a public sample needs months`);
const text = (name) => b64(capsule.files[name]).toString("utf8");
const findings = [];
for (let n = 1; capsule.files[`finding-${n}.txt`]; n++) {
  const m = /^Finding \d+: (.*)\nLocation: (.*)\n\n([\s\S]*?)\n\nHow to check: /.exec(text(`finding-${n}.txt`));
  if (!m) throw new Error(`finding-${n}.txt is not in the form the page reads`);
  findings.push({ title: m[1], where: m[2], why: m[3] });
}
const files = Object.keys(capsule.files).sort().map((name) => {
  const bytes = b64(capsule.files[name]);
  return { name, bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
});
const facts = {
  model: [...models][0],
  model_key: [...signers][0],
  answers: receipts.length,
  size: capsuleBytes.length < 1024 * 1024 ? `${Math.round(capsuleBytes.length / 1024)} KB` : `${(capsuleBytes.length / 1048576).toFixed(1)} MB`,
  expires: new Date(release.expires * 1000).toISOString().slice(0, 10),
  recipient: release.recipient,
  findings,
  files,
};

// the page's script, with the verifier inside it
const require = createRequire(resolve(repo, "apps/web/package.json"));
const { build } = await import(require.resolve("vite"));
const result = await build({
  root: here,
  logLevel: "warn",
  configFile: false,
  resolve: { alias: { "@kakurega/verifier": resolve(repo, "packages/verifier-js/src/index.js"), "@kakurega/canon": resolve(repo, "packages/canon-js/src/index.js") } },
  build: { write: false, minify: true, target: "es2022", lib: { entry: resolve(here, "site.js"), formats: ["iife"], name: "KakuregaSite", fileName: "site" } },
});
const script = (Array.isArray(result) ? result[0] : result).output.find((o) => o.type === "chunk").code;

// the try-it page's script: NEAR's official client for browsers, bundled so the page loads nothing from anywhere else
const tryResult = await build({
  root: here,
  logLevel: "warn",
  configFile: false,
  define: { "process.env.NODE_ENV": '"production"', global: "globalThis" },
  build: { write: false, minify: true, target: "es2022", lib: { entry: resolve(here, "try.js"), formats: ["iife"], name: "KakuregaTry", fileName: "try" } },
});
const tryScript = (Array.isArray(tryResult) ? tryResult[0] : tryResult).output.find((o) => o.type === "chunk").code;

// Kakurega's own secret detector for the browser: the same crate the product uses, with nothing it can import
const wasmBuilt = spawnSync("cargo", ["build", "--quiet", "--release", "--target", "wasm32-unknown-unknown", "--manifest-path", join(here, "wasm/Cargo.toml")], { encoding: "utf8", cwd: repo });
if (wasmBuilt.status !== 0) throw new Error("the detector did not build for the browser (rustup target add wasm32-unknown-unknown): " + wasmBuilt.stderr.slice(-400));
const wasmPath = join(here, "wasm/target/wasm32-unknown-unknown/release/kk_secrets_wasm.wasm");
if (WebAssembly.Module.imports(new WebAssembly.Module(readFileSync(wasmPath))).length !== 0) throw new Error("the detector's browser build imports something; it must import nothing");

rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, "verify"), { recursive: true });
mkdirSync(join(out, "sample"), { recursive: true });
let html = readFileSync(join(here, "index.html"), "utf8");
for (const [key, value] of Object.entries({ MODEL: facts.model, MODEL_KEY: facts.model_key, ANSWERS: String(facts.answers), SIZE: facts.size, EXPIRES: facts.expires })) html = html.replaceAll(`{{${key}}}`, value);
const left = html.match(/\{\{[A-Z_]+\}\}/g);
if (left) throw new Error("the page has places nothing filled: " + left.join(", "));
writeFileSync(join(out, "index.html"), html);
writeFileSync(join(out, "site.js"), script);
writeFileSync(join(out, "try.js"), tryScript);
mkdirSync(join(out, "try"), { recursive: true });
mkdirSync(join(out, "api"), { recursive: true });
cpSync(join(here, "try.html"), join(out, "try/index.html"));
cpSync(wasmPath, join(out, "kk_secrets.wasm"));
cpSync(join(here, "api/near.js"), join(out, "api/near.js"));
writeFileSync(join(out, "package.json"), JSON.stringify({ private: true, type: "module" }));
writeFileSync(join(out, "replay.json"), JSON.stringify(facts));
cpSync(join(here, "site.css"), join(out, "site.css"));
// the check page is one file with its script and its style inside it: the host is told the exact hashes of those two,
// so nothing else can run there even if a capsule's text were ever put into the page as markup
const verifyHtml = readFileSync(join(repo, "apps/verifier-page/dist/kakurega-verify.html"), "utf8");
const inline = (tag) => [...verifyHtml.matchAll(new RegExp(`<${tag}[^>]*>([\\s\\S]*?)</${tag}>`, "g"))].map((m) => `'sha256-${createHash("sha256").update(m[1], "utf8").digest("base64")}'`);
const scripts = inline("script");
const styles = inline("style");
if (scripts.length === 0 || styles.length === 0) throw new Error("the check page has no inline script or style to name in its policy");
const config = JSON.parse(readFileSync(join(here, "vercel.json"), "utf8"));
const rule = config.headers.find((h) => h.source === "/verify(/.*)?")?.headers.find((h) => h.key === "Content-Security-Policy");
if (!rule || !rule.value.includes("script-src 'unsafe-inline'") || !rule.value.includes("style-src 'unsafe-inline'")) throw new Error("vercel.json no longer has the check page's policy in the form this step rewrites");
rule.value = rule.value.replace("script-src 'unsafe-inline'", `script-src ${scripts.join(" ")}`).replace("style-src 'unsafe-inline'", `style-src ${styles.join(" ")}`);
writeFileSync(join(out, "vercel.json"), JSON.stringify(config, null, 2));
cpSync(join(here, "img"), join(out, "img"), { recursive: true });
// the brand's two typefaces (SIL Open Font License), served from this site: no request leaves it for a font
mkdirSync(join(out, "fonts"), { recursive: true });
for (const [pkg, file] of [["outfit", "outfit-latin-400-normal.woff2"], ["outfit", "outfit-latin-500-normal.woff2"], ["outfit", "outfit-latin-600-normal.woff2"], ["outfit", "outfit-latin-700-normal.woff2"], ["jetbrains-mono", "jetbrains-mono-latin-400-normal.woff2"]])
  cpSync(join(repo, "apps/web/node_modules/@fontsource", pkg, "files", file), join(out, "fonts", file));
cpSync(join(repo, "apps/web/public/licenses"), join(out, "fonts/licenses"), { recursive: true });
cpSync(join(repo, "apps/verifier-page/dist/kakurega-verify.html"), join(out, "verify/index.html"));
cpSync(capsulePath, join(out, "sample/review.kkg"));
console.log(`site built in ${out}: ${facts.model}, ${facts.answers} signed answers, ${findings.length} findings, valid until ${facts.expires}`);
