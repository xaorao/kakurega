// The relay behind the site's free tries. It adds the site's own NEAR AI key to a request and passes it on; that is all
// it is for. What a visitor asks is encrypted in their browser to the model's own key before it gets here, so this
// function handles ciphertext: it cannot read a question or an answer, and it keeps and logs nothing.
//
// It is deliberately narrow, because the key behind it spends money:
//   - four paths of NEAR AI Cloud and no others (a model's details, the hardware report, a chat completion, its signature);
//   - one model; an answer of at most MAX_TOKENS; a request of at most MAX_BODY bytes; no streaming;
//   - only from pages of this site (the Origin header), a few chats a day from one address, and a ceiling for all;
//   - without a key in the environment it answers 503 and the page asks the visitor for their own key.
// The counters live in this instance's memory: they slow abuse and are not the real ceiling. The real ceiling is the
// spending limit on the key itself, which is set at NEAR and which this function cannot raise.

const UPSTREAM = "https://cloud-api.near.ai/v1/";
const MODEL = "Qwen/Qwen3.8-27B";
const MAX_TOKENS = 500;
const MAX_BODY = 24 * 1024;
const PER_ADDRESS_A_DAY = 6;
const ALL_A_DAY = 400;
const PATHS = [
  { re: /^model\/Qwen\/Qwen3\.8-27B$/, method: "GET" },
  { re: /^model\/Qwen%2FQwen3\.8-27B$/, method: "GET" },
  { re: /^attestation\/report$/, method: "GET" },
  { re: /^signature\/[A-Za-z0-9._-]{1,160}$/, method: "GET" },
  { re: /^chat\/completions$/, method: "POST" },
];
// what the encrypted-chat protocol needs, and nothing a visitor could use to act as someone else
const PASS_REQUEST = ["content-type", "accept", "x-signing-algo", "x-client-pub-key", "x-model-pub-key", "x-encryption-version", "x-encrypt-all-fields", "x-no-aliasing"];
const PASS_RESPONSE = ["content-type", "x-request-id"];

let day = "";
let all = 0;
const byAddress = new Map();

function count(address) {
  const today = new Date().toISOString().slice(0, 10);
  if (today !== day) {
    day = today;
    all = 0;
    byAddress.clear();
  }
  const mine = byAddress.get(address) ?? 0;
  if (mine >= PER_ADDRESS_A_DAY) return "address";
  if (all >= ALL_A_DAY) return "all";
  byAddress.set(address, mine + 1);
  all += 1;
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
  const url = new URL(req.url, "http://relay");
  const path = url.searchParams.get("path") ?? "";
  url.searchParams.delete("path");
  const rule = PATHS.find((p) => p.re.test(path));
  if (!rule || rule.method !== req.method) return refuse(res, 404, "NOT_RELAYED", "This relay passes on four requests of NEAR AI Cloud and no others.");
  // a page of another site cannot spend this key through a visitor's browser
  const origin = req.headers.origin;
  const host = req.headers["x-forwarded-host"] ?? req.headers.host;
  if (req.method === "POST" && (!origin || new URL(origin).host !== host)) return refuse(res, 403, "OTHER_SITE", "Only this site's own pages may use the free tries.");

  // the hardware report and the model's details are asked about this one model only
  const about = url.searchParams.get("model");
  if (about !== null && about !== MODEL) return refuse(res, 400, "OTHER_MODEL", `Free tries run on ${MODEL} only.`);
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
    // checked, never changed: the model signs the exact bytes it is sent, and the page checks that signature
    if (asked.model !== MODEL) return refuse(res, 400, "OTHER_MODEL", `Free tries run on ${MODEL} only.`);
    if (asked.stream === true) return refuse(res, 400, "NO_STREAM", "Free tries are not streamed.");
    if (!Number.isInteger(asked.max_tokens) || asked.max_tokens < 1 || asked.max_tokens > MAX_TOKENS) return refuse(res, 400, "TOO_LONG", `A free try asks for at most ${MAX_TOKENS} tokens.`);
    if (asked.tools || asked.n > 1) return refuse(res, 400, "NOT_PLAIN", "A free try is one plain answer.");
    const address = String(req.headers["x-forwarded-for"] ?? "unknown").split(",")[0].trim();
    const over = count(address);
    if (over) return refuse(res, 429, over === "address" ? "TRIES_USED" : "ALL_USED", over === "address" ? "Today's free tries from this address are used. Paste your own NEAR AI key to continue." : "Today's free tries are all used. Paste your own NEAR AI key to continue.");
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

export const limits = { MODEL, MAX_TOKENS, MAX_BODY, PER_ADDRESS_A_DAY, ALL_A_DAY };
