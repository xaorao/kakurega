// Reads the Kakurega registry on NEAR from the page itself, through a public RPC. Only called when the reader asks:
// it sends the capsule's digest and the keys named in it to that RPC, and nothing else.

export const NETWORKS = {
  testnet: { rpc: "https://rpc.testnet.fastnear.com", registry: "kakurega-registry-1ba6b2.testnet" },
  mainnet: { rpc: "https://rpc.mainnet.fastnear.com", registry: "" },
};

const ACCOUNT = /^(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$/;
export const isAccount = (s) => typeof s === "string" && s.length >= 2 && s.length <= 64 && ACCOUNT.test(s);
export const isHex = (s, n) => typeof s === "string" && s.length === n && /^[0-9a-f]+$/.test(s);
export const hex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

export async function sha256Hex(bytes) {
  return hex(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)));
}

/** One read-only call. Resolves to the parsed JSON the contract returned; rejects with a short reason in words. */
export async function view(network, registry, method, args, fetcher = fetch) {
  const body = {
    jsonrpc: "2.0",
    id: 1,
    method: "query",
    params: {
      request_type: "call_function",
      finality: "final",
      account_id: registry,
      method_name: method,
      args_base64: btoa(JSON.stringify(args)),
    },
  };
  let res;
  try {
    res = await fetcher(network.rpc, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
      credentials: "omit",
      referrerPolicy: "no-referrer",
      redirect: "error",
    });
  } catch {
    throw new Error("The NEAR RPC could not be reached.");
  }
  if (!res.ok) throw new Error(`The NEAR RPC answered ${res.status}.`);
  let doc;
  try {
    doc = await res.json();
  } catch {
    throw new Error("The NEAR RPC's answer is not JSON.");
  }
  const result = doc && doc.result;
  if (!result || !Array.isArray(result.result)) {
    throw new Error("That account does not answer as a Kakurega registry.");
  }
  if (result.result.length > 65536) throw new Error("The registry's answer is larger than expected.");
  try {
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(result.result)));
  } catch {
    throw new Error("The registry's answer could not be read.");
  }
}

/** Chain time (nanoseconds since 1970, as a string) in words. */
export function when(nanos) {
  let n;
  try {
    n = BigInt(nanos);
  } catch {
    return "an unreadable time";
  }
  const d = new Date(Number(n / 1000000n));
  return Number.isNaN(d.getTime()) ? "an unreadable time" : d.toISOString().replace("T", " ").slice(0, 19) + " UTC";
}

/** An order's amount in words: NEAR to five places, or the token's smallest units. */
export function amount(order) {
  let raw;
  try {
    raw = BigInt(order.amount);
  } catch {
    return "an unreadable amount";
  }
  if (order.token === null || order.token === undefined) {
    const unit = 10n ** 24n;
    const part = ((raw % unit) / 10n ** 19n).toString().padStart(5, "0");
    return `${raw / unit}.${part} NEAR`;
  }
  return `${raw} smallest units of token ${order.token}`;
}

/**
 * Everything the registry holds about one capsule: its entry, the hardware record of each model key named in the capsule,
 * and the order it was registered for. `receipts` are the capsule's own verified receipts.
 */
export async function lookup(network, registry, capsuleSha256, signerHex, receipts, fetcher = fetch) {
  if (!isAccount(registry)) throw new Error("That is not a NEAR account name.");
  if (!isHex(capsuleSha256, 64) || !isHex(signerHex, 64)) throw new Error("The capsule's digest or key is not in the expected form.");
  const call = (method, args) => view(network, registry, method, args, fetcher);
  const entry = await call("get", { capsule_sha256: capsuleSha256, capsule_signer: signerHex });
  const keys = [...new Set(receipts.map((r) => r.signer).filter((k) => isHex(k, 64)))].slice(0, 64);
  const hardware = [];
  for (const k of keys) hardware.push({ signer: k, record: await call("hardware", { signer: k }) });
  let order = null;
  if (entry && typeof entry.order === "string" && /^\d{1,20}$/.test(entry.order)) {
    order = { id: entry.order, order: await call("order", { order_id: entry.order }) };
  }
  const { count, unregistered, different } = await matchReceipts(entry, receipts);
  return { entry, hardware, order, count, unregistered, different };
}

/** SHA-256, in hex, of receipts as the registry reads them: one line each, `signer:model:request:response:signature`. */
export async function receiptsDigest(receipts) {
  const text = receipts.map((r) => `${r.signer}:${r.model}:${r.requestSha256}:${r.responseSha256}:${r.signature}\n`).join("");
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Whether the model signatures registered with an entry are this capsule's own. A registry that keeps their digest is compared
 * with the digest of the capsule's first `receipt_count` receipts (the ones a registration carries); an older one, which kept
 * each receipt, is compared receipt by receipt.
 */
export async function matchReceipts(entry, receipts) {
  if (!entry) return { count: 0, unregistered: 0, different: false };
  if (typeof entry.receipts_sha256 === "string") {
    const count = Number.isSafeInteger(entry.receipt_count) ? entry.receipt_count : 0;
    if (count > receipts.length || (await receiptsDigest(receipts.slice(0, count))) !== entry.receipts_sha256) return { count, unregistered: 0, different: true };
    return { count, unregistered: receipts.length - count, different: false };
  }
  const onChain = Array.isArray(entry.receipts) ? entry.receipts : [];
  const unregistered = receipts.filter(
    (r) => !onChain.some((c) => c.signer === r.signer && c.request_sha256 === r.requestSha256 && c.response_sha256 === r.responseSha256),
  ).length;
  return { count: onChain.length, unregistered, different: false };
}
