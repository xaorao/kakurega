// the relay's refusals, exercised without any network: no key, wrong path, other site, other model, too long, too many
import handler, { limits } from "../../apps/site/api/near.js";
import { Readable } from "node:stream";
const call = async (method, path, { headers = {}, body, key = "test-key" } = {}) => {
  if (key) process.env.NEAR_AI_RELAY_KEY = key; else delete process.env.NEAR_AI_RELAY_KEY;
  const req = body === undefined ? Readable.from([]) : Readable.from([Buffer.from(typeof body === "string" ? body : JSON.stringify(body))]);
  req.method = method; req.url = "/api/near?path=" + path; req.headers = { host: "site.example", ...headers };
  const res = { headers: {}, setHeader(k, v) { this.headers[k] = v; }, end(b) { this.body = b?.toString?.() ?? ""; } };
  await handler(req, res);
  return { status: res.statusCode, code: (() => { try { return JSON.parse(res.body).error.code; } catch { return null; } })() };
};
const ok = { model: limits.MODEL, max_tokens: 100, messages: [{ role: "user", content: "sealed-text" }] };
// what the encrypted-chat protocol's client sends with a sealed question
const sealed = { "x-encryption-version": "3", "x-encrypt-all-fields": "true", "x-client-pub-key": "aa", "x-model-pub-key": "bb", "x-signing-algo": "ed25519" };
const same = { origin: "https://site.example", "x-forwarded-for": "203.0.113.9", ...sealed };
const out = [];
const expect = async (name, p, want) => { const r = await p; out.push(`${r.status === want[0] && r.code === want[1] ? "PASS" : "FAIL"} ${name}: ${r.status} ${r.code}`); };
// fetch is replaced so nothing leaves: it records what would be sent
let sent = [];
globalThis.fetch = async (u, init) => { sent.push({ u, auth: init.headers.authorization, hasClientAuth: Object.keys(init.headers).filter((k) => /api-key|cookie/i.test(k)) }); return new Response("{}", { status: 200, headers: { "content-type": "application/json" } }); };
await expect("no key in the environment", call("GET", "attestation/report", { key: null }), [503, "NO_FREE_TRIES"]);
await expect("a path that is not relayed", call("GET", "files"), [404, "NOT_RELAYED"]);
await expect("a path that tries to climb", call("GET", "attestation/report/../../models"), [404, "NOT_RELAYED"]);
await expect("the right path with the wrong method", call("POST", "attestation/report", { headers: same, body: ok }), [404, "NOT_RELAYED"]);
await expect("a chat from another site's page", call("POST", "chat/completions", { headers: { origin: "https://evil.example" }, body: ok }), [403, "OTHER_SITE"]);
await expect("a chat with no Origin at all", call("POST", "chat/completions", { body: ok }), [403, "OTHER_SITE"]);
await expect("another model", call("POST", "chat/completions", { headers: same, body: { ...ok, model: "openai/gpt" } }), [400, "OTHER_MODEL"]);
await expect("the hardware report of another model", call("GET", "attestation/report%26model=x&model=other"), [404, "NOT_RELAYED"]);
await expect("no limit on the answer", call("POST", "chat/completions", { headers: same, body: { model: limits.MODEL, messages: ok.messages } }), [400, "TOO_LONG"]);
// the ways round the limits that a model server would honour: each is a field this relay does not pass on
for (const [name, extra] of [["another name for the token limit", { max_completion_tokens: 100000 }], ["several answers at once", { n: 8 }], ["best_of", { best_of: 20 }], ["functions", { functions: [{}] }], ["a tool choice", { tool_choice: "auto" }], ["logprobs", { logprobs: true, top_logprobs: 20 }], ["a field nobody has heard of", { whatever: 1 }]])
  await expect(name, call("POST", "chat/completions", { headers: same, body: { ...ok, ...extra } }), [400, "NOT_PLAIN"]);
await expect("stream given as 1, not true", call("POST", "chat/completions", { headers: same, body: { ...ok, stream: 1 } }), [400, "NO_STREAM"]);
await expect("a body that is null", call("POST", "chat/completions", { headers: same, body: "null" }), [400, "NOT_PLAIN"]);
await expect("a body that is a list", call("POST", "chat/completions", { headers: same, body: "[]" }), [400, "NOT_PLAIN"]);
await expect("no messages", call("POST", "chat/completions", { headers: same, body: { ...ok, messages: [] } }), [400, "NOT_PLAIN"]);
await expect("more messages than a short question needs", call("POST", "chat/completions", { headers: same, body: { ...ok, messages: Array(limits.MAX_MESSAGES + 1).fill(ok.messages[0]) } }), [400, "NOT_PLAIN"]);
await expect("a message that carries more than text", call("POST", "chat/completions", { headers: same, body: { ...ok, messages: [{ role: "user", content: [{ type: "image_url", image_url: "x" }] }] } }), [400, "NOT_PLAIN"]);
await expect("a message with a tool call in it", call("POST", "chat/completions", { headers: same, body: { ...ok, messages: [{ role: "assistant", content: "x", tool_calls: [] }] } }), [400, "NOT_PLAIN"]);
await expect("thinking switched on through the template", call("POST", "chat/completions", { headers: same, body: { ...ok, chat_template_kwargs: { enable_thinking: false, other: 1 } } }), [400, "NOT_PLAIN"]);
await expect("a question that is not marked as encrypted", call("POST", "chat/completions", { headers: { origin: "https://site.example", "x-forwarded-for": "203.0.113.9" }, body: ok }), [400, "NOT_ENCRYPTED"]);
await expect("an older encryption version", call("POST", "chat/completions", { headers: { ...same, "x-encryption-version": "1" }, body: ok }), [400, "NOT_ENCRYPTED"]);
await expect("an Origin of null", call("POST", "chat/completions", { headers: { ...same, origin: "null" }, body: ok }), [403, "OTHER_SITE"]);
await expect("an Origin that is not an address", call("POST", "chat/completions", { headers: { ...same, origin: "::::" }, body: ok }), [403, "OTHER_SITE"]);
await expect("a read made by a page of another site", call("GET", "attestation/report", { headers: { origin: "https://evil.example" } }), [403, "OTHER_SITE"]);
await expect("a read a browser marks as cross-site", call("GET", "attestation/report", { headers: { "sec-fetch-site": "cross-site" } }), [403, "OTHER_SITE"]);
await expect("a read with the model named twice", call("GET", "attestation/report&model=" + encodeURIComponent(limits.MODEL) + "&model=other"), [400, "BAD_REQUEST"]);
await expect("a read with a field the protocol does not use", call("GET", "attestation/report&redirect=https://evil.example"), [400, "BAD_REQUEST"]);
await expect("a signature path made of dots", call("GET", "signature/.."), [404, "NOT_RELAYED"]);
await expect("a signature path of one dot", call("GET", "signature/."), [404, "NOT_RELAYED"]);
await expect("an answer longer than allowed", call("POST", "chat/completions", { headers: same, body: { ...ok, max_tokens: limits.MAX_TOKENS + 1 } }), [400, "TOO_LONG"]);
await expect("a streamed answer", call("POST", "chat/completions", { headers: same, body: { ...ok, stream: true } }), [400, "NO_STREAM"]);
await expect("tools", call("POST", "chat/completions", { headers: same, body: { ...ok, tools: [{}] } }), [400, "NOT_PLAIN"]);
await expect("a body over the size limit", call("POST", "chat/completions", { headers: same, body: JSON.stringify({ ...ok, pad: "x".repeat(limits.MAX_BODY) }) }), [413, "TOO_LARGE"]);
await expect("text that is not JSON", call("POST", "chat/completions", { headers: same, body: "not json" }), [400, "NOT_JSON"]);
sent = [];
for (let i = 0; i < limits.PER_ADDRESS_A_DAY; i++) await expect(`free try ${i + 1} is passed on`, call("POST", "chat/completions", { headers: { ...same, authorization: "Bearer visitor-key", "x-api-key": "v", cookie: "c=1" }, body: ok }), [200, null]);
await expect("one more from the same address", call("POST", "chat/completions", { headers: same, body: ok }), [429, "TRIES_USED"]);
out.push(`${sent.length === limits.PER_ADDRESS_A_DAY && sent.every((s) => s.auth === "Bearer test-key" && s.hasClientAuth.length === 0 && s.u === "https://cloud-api.near.ai/v1/chat/completions") ? "PASS" : "FAIL"} what is passed on carries the site's key, never the visitor's headers, to the one address: ${JSON.stringify(sent[0])}`);
sent = [];
await expect("the model's details", call("GET", "model/Qwen/Qwen3.8-27B"), [200, null]);
out.push(`${sent[0]?.u === "https://cloud-api.near.ai/v1/model/Qwen%2FQwen3.8-27B" ? "PASS" : "FAIL"} the model's name goes on as one path segment: ${sent[0]?.u}`);
sent = [];
for (let i = 0; i < limits.READS_PER_ADDRESS_A_DAY; i++) await call("GET", "attestation/report", { headers: { "x-forwarded-for": "198.51.100.7" } });
await expect("reads from one address stop at their own limit", call("GET", "attestation/report", { headers: { "x-forwarded-for": "198.51.100.7" } }), [429, "TRIES_USED"]);
out.push(`${sent.length === limits.READS_PER_ADDRESS_A_DAY ? "PASS" : "FAIL"} and exactly that many were passed on: ${sent.length}`);
console.log(out.join("\n"));
if (out.some((l) => l.startsWith("FAIL"))) process.exit(1);
