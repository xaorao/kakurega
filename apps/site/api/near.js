// The relay behind the site's free tries. It adds the site's own NEAR AI key to a request and passes it on; that is all
// it is for. What a visitor asks is encrypted in their browser to the model's own key before it gets here, so this
// function handles ciphertext: it cannot read a question or an answer, and it keeps and logs nothing.
//
// It is deliberately narrow, because the key behind it spends money:
//   - four paths of NEAR AI Cloud and no others (a model's details, the hardware report, a chat completion, its signature);
//   - one model; an answer of at most MAX_TOKENS; a request of at most MAX_BODY bytes; no streaming;
//   - a chat must be an encrypted one (the protocol's headers are required), and its body may hold only the fields a
//     plain question needs: anything else a model server might act on (other token limits, tools, several answers) is
//     refused, not passed on;
//   - only from pages of this site (the Origin header, which a browser sets and a script can forge), a few chats a day
//     from one address, a ceiling for all, and a looser count on the read-only requests;
//   - without a key in the environment it answers 503 and the page asks the visitor for their own key.
// The counters live in this instance's memory, and a host runs several instances: they slow abuse and are NOT the
// ceiling. The ceiling is the balance of the key itself. Only a key with a small fixed balance belongs behind this.

const UPSTREAM = "https://cloud-api.near.ai/v1/";
const MODEL = "Qwen/Qwen3.8-27B";
const MAX_TOKENS = 500;
const MAX_BODY = 24 * 1024;
const PER_ADDRESS_A_DAY = 6;
const ALL_A_DAY = 400;
// one try makes about four read-only requests (the model's details, the hardware report, the signature)
const READS_PER_ADDRESS_A_DAY = 60;
const READS_ALL_A_DAY = 6000;
// the only fields a chat's body may hold, and the only query fields a read may carry
const BODY_FIELDS = ["model", "messages", "max_tokens", "chat_template_kwargs", "stream"];
const QUERY_FIELDS = ["model", "provider", "nonce", "include_tls_fingerprint", "signing_algo", "signing_address"];
const MAX_MESSAGES = 4;
const PATHS = [
  { re: /^model\/Qwen\/Qwen3\.8-27B$/, method: "GET" },
  { re: /^model\/Qwen%2FQwen3\.8-27B$/, method: "GET" },
  { re: /^attestation\/report$/, method: "GET" },
  { re: /^signature\/(?!\.+$)[A-Za-z0-9._-]{1,160}$/, method: "GET" },
  { re: /^chat\/completions$/, method: "POST" },
];
// what the encrypted-chat protocol needs, and nothing a visitor could use to act as someone else
const PASS_REQUEST = ["content-type", "accept", "x-signing-algo", "x-client-pub-key", "x-model-pub-key", "x-encryption-version", "x-encrypt-all-fields", "x-no-aliasing"];
const PASS_RESPONSE = ["content-type", "x-request-id"];

let day = "";
const totals = { chat: 0, read: 0 };
const byAddress = { chat: new Map(), read: new Map() };
const LIMITS = { chat: [PER_ADDRESS_A_DAY, ALL_A_DAY], read: [READS_PER_ADDRESS_A_DAY, READS_ALL_A_DAY] };

function count(kind, address) {
  const today = new Date().toISOString().slice(0, 10);
  if (today !== day) {
    day = today;
    totals.chat = totals.read = 0;
    byAddress.chat.clear();
    byAddress.read.clear();
  }
  const [each, everyone] = LIMITS[kind];
  const mine = byAddress[kind].get(address) ?? 0;
  if (mine >= each) return "address";
  if (totals[kind] >= everyone) return "all";
  byAddress[kind].set(address, mine + 1);
  totals[kind] += 1;
  return null;
}

/** Why a chat's body is refused, or null. Checked and never changed: the model signs the exact bytes it is sent. */
function wrongBody(asked) {
  if (asked === null || typeof asked !== "object" || Array.isArray(asked)) return ["NOT_PLAIN", "The request is not a chat."];
  const extra = Object.keys(asked).find((k) => !BODY_FIELDS.includes(k));
  if (extra !== undefined) return ["NOT_PLAIN", "A free try is one plain answer: a field of this request is not passed on."];
  if (asked.model !== MODEL) return ["OTHER_MODEL", `Free tries run on ${MODEL} only.`];
  if ("stream" in asked && asked.stream !== false) return ["NO_STREAM", "Free tries are not streamed."];
  if (!Number.isInteger(asked.max_tokens) || asked.max_tokens < 1 || asked.max_tokens > MAX_TOKENS) return ["TOO_LONG", `A free try asks for at most ${MAX_TOKENS} tokens.`];
  if (!Array.isArray(asked.messages) || asked.messages.length < 1 || asked.messages.length > MAX_MESSAGES) return ["NOT_PLAIN", "A free try is one short conversation."];
  for (const m of asked.messages) {
    if (m === null || typeof m !== "object" || Array.isArray(m) || Object.keys(m).some((k) => k !== "role" && k !== "content") || typeof m.role !== "string" || typeof m.content !== "string") return ["NOT_PLAIN", "A free try's messages are text only."];
  }
  if ("chat_template_kwargs" in asked) {
    const k = asked.chat_template_kwargs;
    if (k === null || typeof k !== "object" || Array.isArray(k) || Object.keys(k).some((n) => n !== "enable_thinking")) return ["NOT_PLAIN", "A free try is one plain answer."];
  }
  return null;
}

function refuse(res, status, code, message) {
  res.statusCode = status;
  res.setHeader("content-type", "application/json; charset=utf-8");
  res.setHeader("cache-control", "no-store");
  res.end(JSON.stringify({ error: { code, message } }));
}

async function readBody(req) {
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > MAX_BODY) return null;
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

export default async function handler(req, res) {
  const key = process.env.NEAR_AI_RELAY_KEY;
  if (!key) return refuse(res, 503, "NO_FREE_TRIES", "Free tries are switched off. Paste your own NEAR AI key to continue.");
  let url;
  try {
    url = new URL(req.url, "http://relay");
  } catch {
    return refuse(res, 400, "BAD_REQUEST", "The address of this request cannot be read.");
  }
  // a host that adds a slash to the end of an address does not change which request this is
  const path = (url.searchParams.get("path") ?? "").replace(/\/+$/, "");
  url.searchParams.delete("path");
  const rule = PATHS.find((p) => p.re.test(path));
  if (!rule || rule.method !== req.method) return refuse(res, 404, "NOT_RELAYED", "This relay passes on four requests of NEAR AI Cloud and no others.");
  // a page of another site cannot spend this key through a visitor's browser
  const origin = req.headers.origin;
  const host = req.headers["x-forwarded-host"] ?? req.headers.host;
  let originHost = null;
  try {
    originHost = origin ? new URL(origin).host : null;
  } catch {
    originHost = null;
  }
  // a browser names the page a POST comes from, and marks a read made by a page of another site
  if (req.method === "POST" && originHost !== host) return refuse(res, 403, "OTHER_SITE", "Only this site's own pages may use the free tries.");
  if (req.method === "GET" && ((origin && originHost !== host) || req.headers["sec-fetch-site"] === "cross-site")) return refuse(res, 403, "OTHER_SITE", "Only this site's own pages may use the free tries.");

  // a read carries only the query fields the protocol uses, each once, and asks about this one model only
  const names = [...url.searchParams.keys()];
  if (names.some((n) => !QUERY_FIELDS.includes(n)) || new Set(names).size !== names.length) return refuse(res, 400, "BAD_REQUEST", "A field of this request is not passed on.");
  const about = url.searchParams.get("model");
  if (about !== null && about !== MODEL) return refuse(res, 400, "OTHER_MODEL", `Free tries run on ${MODEL} only.`);
  const address = String(req.headers["x-forwarded-for"] ?? "unknown").split(",")[0].trim();
  // the model's name travels as one path segment, however the host handed it over
  const upstreamPath = path.startsWith("model/") ? "model/" + encodeURIComponent(MODEL) : path;

  let body;
  if (req.method === "POST") {
    body = await readBody(req);
    if (body === null) return refuse(res, 413, "TOO_LARGE", `A free try takes at most ${MAX_BODY} bytes.`);
    let asked;
    try {
      asked = JSON.parse(body.toString("utf8"));
    } catch {
      return refuse(res, 400, "NOT_JSON", "The request is not JSON.");
    }
    const wrong = wrongBody(asked);
    if (wrong) return refuse(res, 400, wrong[0], wrong[1]);
    // an encrypted chat only: the question is sealed in the visitor's browser to the model's key, and NEAR AI Cloud
    // refuses a request that carries these headers and is not sealed
    if (req.headers["x-encryption-version"] !== "3" || req.headers["x-encrypt-all-fields"] !== "true" || !req.headers["x-client-pub-key"] || !req.headers["x-model-pub-key"] || !req.headers["x-signing-algo"]) return refuse(res, 400, "NOT_ENCRYPTED", "A free try is an encrypted question; this one is not marked as one.");
    const over = count("chat", address);
    if (over) return refuse(res, 429, over === "address" ? "TRIES_USED" : "ALL_USED", over === "address" ? "Today's free tries from this address are used. Paste your own NEAR AI key to continue." : "Today's free tries are all used. Paste your own NEAR AI key to continue.");
  } else {
    const over = count("read", address);
    if (over) return refuse(res, 429, over === "address" ? "TRIES_USED" : "ALL_USED", "Today's free tries are used. Paste your own NEAR AI key to continue.");
  }

  const headers = { authorization: `Bearer ${key}` };
  for (const name of PASS_REQUEST) if (req.headers[name]) headers[name] = req.headers[name];
  let upstream;
  try {
    upstream = await fetch(UPSTREAM + upstreamPath + (url.search || ""), { method: req.method, headers, body, redirect: "error", signal: AbortSignal.timeout(120000) });
  } catch {
    return refuse(res, 502, "UPSTREAM", "NEAR AI Cloud could not be reached.");
  }
  const bytes = Buffer.from(await upstream.arrayBuffer());
  res.statusCode = upstream.status;
  for (const name of PASS_RESPONSE) {
    const v = upstream.headers.get(name);
    if (v) res.setHeader(name, v);
  }
  res.setHeader("cache-control", "no-store");
  res.end(bytes);
}

export const limits = { MODEL, MAX_TOKENS, MAX_BODY, PER_ADDRESS_A_DAY, ALL_A_DAY, READS_PER_ADDRESS_A_DAY, READS_ALL_A_DAY, BODY_FIELDS, QUERY_FIELDS, MAX_MESSAGES };
