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

rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, "verify"), { recursive: true });
mkdirSync(join(out, "sample"), { recursive: true });
let html = readFileSync(join(here, "index.html"), "utf8");
for (const [key, value] of Object.entries({ MODEL: facts.model, MODEL_KEY: facts.model_key, ANSWERS: String(facts.answers), SIZE: facts.size, EXPIRES: facts.expires })) html = html.replaceAll(`{{${key}}}`, value);
const left = html.match(/\{\{[A-Z_]+\}\}/g);
if (left) throw new Error("the page has places nothing filled: " + left.join(", "));
writeFileSync(join(out, "index.html"), html);
writeFileSync(join(out, "site.js"), script);
writeFileSync(join(out, "replay.json"), JSON.stringify(facts));
cpSync(join(here, "site.css"), join(out, "site.css"));
cpSync(join(here, "vercel.json"), join(out, "vercel.json"));
cpSync(join(here, "img"), join(out, "img"), { recursive: true });
cpSync(join(repo, "apps/verifier-page/dist/kakurega-verify.html"), join(out, "verify/index.html"));
cpSync(capsulePath, join(out, "sample/review.kkg"));
console.log(`site built in ${out}: ${facts.model}, ${facts.answers} signed answers, ${findings.length} findings, valid until ${facts.expires}`);
