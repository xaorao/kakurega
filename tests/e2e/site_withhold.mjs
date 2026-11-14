// What the try-it page would send, worked out with the real detector (the WebAssembly build the site serves) and the
// page's own withholding step, with no browser and no network. The rule under test: no part of a secret is in the text
// that leaves, whatever its shape or place.
//   node tests/e2e/site_withhold.mjs
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const wasmPath = join(repo, "apps/site/dist/kk_secrets.wasm");
if (!existsSync(wasmPath) || process.env.SITE_REBUILD !== "0") {
  const built = spawnSync("node", [join(repo, "apps/site/build.mjs")], { encoding: "utf8" });
  if (built.status !== 0) throw new Error(built.stderr);
}
const { withhold } = await import(pathToFileURL(join(repo, "apps/site/withhold.js")).href);
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const x = instance.exports;
const scan = (text) => {
  const bytes = new TextEncoder().encode(text);
  const p = x.kk_alloc(bytes.length);
  new Uint8Array(x.memory.buffer, p, bytes.length).set(bytes);
  const answer = x.kk_scan(p, bytes.length);
  const len = new DataView(x.memory.buffer).getUint32(answer, true);
  const json = new TextDecoder().decode(new Uint8Array(x.memory.buffer, answer + 4, len));
  x.kk_free(p, bytes.length);
  x.kk_free(answer, 4 + len);
  return JSON.parse(json);
};
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const sentOf = (text) => withhold(text, scan(text));

// made-up material, put together here so no key-shaped text sits in this file
const BEGIN = (kind) => "-----BEGIN " + kind + " PRIVATE KEY-----";
const END = (kind) => "-----END " + kind + " PRIVATE KEY-----";
const body = ["b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ", "QyNTUxOQAAACDx7Yk2mPq9RrT4wZbN3cD6", "AAAAQH8sV1nLk0pQ2jXv9Rt4Yw7Bn3Cd6eF"];
// an access key ID uses only A to Z and 2 to 7; a made-up one in that alphabet
const aws = "AKIA" + "Q7XK2M5P" + "LD4RT6WZ";

// 1. a private key block: every line of it is withheld, not only the first
let r = sentOf(["host = 'db1'", BEGIN("OPENSSH"), ...body, END("OPENSSH"), "port = 5432"].join("\n"));
check(body.every((b) => !r.sent.includes(b)) && !r.sent.includes("PRIVATE KEY-----"), "no line of a private key block is in what is sent");
check(r.held.length === 5 && r.sent.includes("host = 'db1'") && r.sent.includes("port = 5432"), "the five lines of the block are withheld and the lines around it are kept: " + r.held.map((h) => h.line).join(","));

// 2. a block that never ends, after text in another script and an emoji (offsets are bytes)
r = sentOf(["# नमस्ते 🔒 चाबी नीचे है", BEGIN("RSA"), ...body].join("\n"));
check(body.every((b) => !r.sent.includes(b)) && r.sent.startsWith("# नमस्ते 🔒 चाबी नीचे है\n"), "a block with no end line is withheld to the end, and the line before it is kept whole");

// 3. two blocks, with a kept line between them
r = sentOf([BEGIN("EC"), body[0], END("EC"), "keep me", BEGIN("EC"), body[1], END("EC")].join("\n"));
check(!r.sent.includes(body[0]) && !r.sent.includes(body[1]) && r.sent.split("\n")[3] === "keep me", "a second block is found too");

// 4. a key at the very end with no newline, and one on a line with Windows line ends
r = sentOf("a = 1\r\nAWS = '" + aws + "'\r\nlast = '" + aws + "'");
check(!r.sent.includes(aws), "a key on a CRLF line and a key on the last line with no newline are both withheld");

// 5. a password said as a password
r = sentOf("notes\nthe database password is Tuesday-maple-42, do not share it\nend");
check(!r.sent.includes("Tuesday-maple-42") && r.sent.includes("notes") && r.sent.includes("end"), "a password said as a password is withheld");

// 6. indented, as in a YAML file
r = sentOf(["deploy:", "  key: |", "    " + BEGIN("OPENSSH"), ...body.map((b) => "    " + b), "    " + END("OPENSSH"), "  user: ci"].join("\n"));
check(body.every((b) => !r.sent.includes(b)) && r.sent.includes("user: ci"), "an indented block is withheld whole");

// 7. nothing secret: nothing is withheld, the text is sent as it is
const plain = "def add(a, b):\n    return a + b\n-----BEGIN PUBLIC KEY-----\nMFkw\n-----END PUBLIC KEY-----\n";
r = sentOf(plain);
check(r.held.length === 0 && r.sent === plain, "code with no secret, and a public key, are sent unchanged");

// 8. the page's own sample
const sample = readFileSync(join(repo, "apps/site/try.js"), "utf8");
check(/import \{ withhold \} from "\.\/withhold\.js"/.test(sample), "the page uses this same withholding step");
