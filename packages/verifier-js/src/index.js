import { parseStrict, serialize, CanonError } from "@kakurega/canon";
import { classifyPublicKey, verifySignature } from "./ed25519.js";

const RELEASE_TYPE = "application/vnd.kakurega.release.v1+json";
const CLAIM_TYPE = "application/vnd.kakurega.claim.v1+json";
const MAX_FILES = 256;
const MAX_TOTAL = 64 * 1024 * 1024;
const MAX_RECEIPTS = 64;
const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const enc = new TextEncoder();

class Stop extends Error {
  constructor(reason) {
    super(reason);
    this.reason = reason;
  }
}
const stop = (reason) => {
  throw new Stop(reason);
};

export function b64urlEncode(bytes) {
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0);
    out += ALPHABET[(n >> 18) & 63] + ALPHABET[(n >> 12) & 63];
    if (i + 1 < bytes.length) out += ALPHABET[(n >> 6) & 63];
    if (i + 2 < bytes.length) out += ALPHABET[n & 63];
  }
  return out;
}

export function b64urlDecode(text) {
  if (text.length % 4 === 1) return null;
  const out = [];
  for (let i = 0; i < text.length; i += 4) {
    const chunk = text.slice(i, i + 4);
    let n = 0;
    for (let j = 0; j < chunk.length; j++) {
      const v = ALPHABET.indexOf(chunk[j]);
      if (v < 0) return null;
      n |= v << (18 - 6 * j);
    }
    out.push((n >> 16) & 255);
    if (chunk.length > 2) out.push((n >> 8) & 255);
    if (chunk.length > 3) out.push(n & 255);
    const leftover = chunk.length === 2 ? n & 0xffff : chunk.length === 3 ? n & 0xff : 0;
    if (leftover !== 0) return null;
  }
  return Uint8Array.from(out);
}

async function sha256Hex(bytes) {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}

const isMap = (v) => v instanceof Map;
const exactKeys = (v, names) => isMap(v) && v.size === names.length && names.every((n) => v.has(n));
const isStr = (v) => typeof v === "string";
const isInt = (v) => typeof v === "bigint";
const isHex64 = (x) => /^[0-9a-f]{64}$/.test(x);
const validFileName = (n) => /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(n) && !n.includes("..");

function pae(type, payload) {
  const typeBytes = enc.encode(type);
  const head = enc.encode(`DSSEv1 ${typeBytes.length} ${type} ${payload.length} `);
  const out = new Uint8Array(head.length + payload.length);
  out.set(head, 0);
  out.set(payload, head.length);
  return out;
}

async function keyIdOf(pub) {
  return "ed25519:" + (await sha256Hex(pub));
}

function parseOrStop(input, reason) {
  try {
    return parseStrict(input);
  } catch (e) {
    if (e instanceof CanonError) stop(reason);
    throw e;
  }
}

export async function verifyCapsule(bytes, options = {}) {
  const { pinned = null, now = 0, recipient: expectedRecipient = null } = options;
  try {
    return await run(bytes, pinned, BigInt(now), expectedRecipient);
  } catch (e) {
    if (e instanceof Stop) return { valid: false, reason: e.reason, signer: null };
    throw e;
  }
}

async function run(bytes, pinned, now, expectedRecipient) {
  if (bytes.length > MAX_TOTAL * 2) stop("capsule_too_large");
  const root = parseOrStop(bytes, "capsule_not_json");
  const text = serialize(root);
  const canonical = enc.encode(text);
  if (canonical.length !== bytes.length || !canonical.every((b, i) => b === bytes[i])) stop("capsule_not_canonical");
  if (!exactKeys(root, ["envelopes", "files", "format", "keys", "version"])) stop("capsule_shape");
  if (root.get("format") !== "kakurega-capsule") stop("capsule_shape");
  if (root.get("version") !== 1n) stop("unsupported_version");
  const keys = root.get("keys");
  const envs = root.get("envelopes");
  const files = root.get("files");
  if (!Array.isArray(keys) || !Array.isArray(envs) || !isMap(files)) stop("capsule_shape");

  const claimed = [];
  for (const k of keys) {
    if (!isStr(k)) stop("bad_key");
    const raw = b64urlDecode(k);
    if (raw === null || raw.length !== 32) stop("bad_key");
    claimed.push(raw);
  }
  const isPinned = pinned !== null;
  const source = isPinned ? pinned : claimed;
  const trusted = new Map();
  for (const k of source) {
    if (classifyPublicKey(k) !== "ok") stop("bad_key");
    trusted.set(await keyIdOf(k), k);
  }
  if (trusted.size === 0) stop("no_keys");

  if (envs.length === 0 || envs.length > 64) stop("envelope_count");
  let release = null;
  const claims = [];
  let signer = null;
  for (const e of envs) {
    const env = decodeEnvelope(e);
    if (env === null) stop("bad_envelope");
    if (env.type !== RELEASE_TYPE && env.type !== CLAIM_TYPE) stop("unknown_payload_type");
    if (env.signatures.length !== 1) stop("signature_count");
    const [keyid, sig] = env.signatures[0];
    const pub = trusted.get(keyid);
    if (pub === undefined) stop(isPinned ? "untrusted_signer" : "unknown_signer");
    if (!(await verifySignature(pub, pae(env.type, env.payload), sig))) stop("bad_signature");
    let payloadText;
    try {
      payloadText = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(env.payload);
    } catch {
      stop("payload_not_canonical");
    }
    let canonicalPayload;
    try {
      canonicalPayload = serialize(parseStrict(payloadText));
    } catch {
      stop("payload_not_canonical");
    }
    if (canonicalPayload !== payloadText) stop("payload_not_canonical");
    if (signer === null) signer = keyid;
    else if (signer !== keyid) stop("mixed_signers");
    const value = parseOrStop(payloadText, "payload_not_canonical");
    if (env.type === RELEASE_TYPE) {
      if (release !== null) stop("duplicate_release");
      release = { env, value };
    } else {
      claims.push(value);
    }
  }
  if (release === null) stop("release_missing");
  // The key list is not covered by any signature, so it must be exactly the one key that signed.
  if (claimed.length !== 1 || (await keyIdOf(claimed[0])) !== signer) stop("key_list");
  const rel = release.value;

  // A release names a digest of each of its claims, in order, so that a claim taken out, added or swapped is seen. One made
  // before that has no such list: it still verifies, and the result says what that leaves open.
  const named = rel.has("claims") ? rel.get("claims") : null;
  if (named !== null && (!Array.isArray(named) || !named.every((d) => isStr(d) && isHex64(d)))) stop("release_shape");
  const releaseKeys = ["type", "version", "capsule_id", "created", "expires", "recipient", "policy_version", "files", "assurance", "source_commitment"];
  if (!exactKeys(rel, named === null ? releaseKeys : ["claims", ...releaseKeys]) || rel.get("type") !== "release") stop("release_shape");
  if (rel.get("version") !== 1n) stop("unsupported_version");
  const created = rel.get("created");
  const expires = rel.get("expires");
  if (!isInt(created) || !isInt(expires)) stop("release_shape");
  const capsuleId = rel.get("capsule_id");
  const recipient = rel.get("recipient");
  const policyVersion = rel.get("policy_version");
  if (!isStr(capsuleId) || !isStr(recipient) || !isStr(policyVersion)) stop("release_shape");
  if (capsuleId === "" || recipient === "" || policyVersion === "" || expires <= created) stop("release_shape");
  const assurance = rel.get("assurance");
  const commit = rel.get("source_commitment");
  const withReceipts = isMap(assurance) && assurance.has("route_receipts");
  if (!(exactKeys(assurance, ["binding_level", "route", "not_established"]) || exactKeys(assurance, ["binding_level", "route", "not_established", "route_receipts"])) || !exactKeys(commit, ["scheme", "digest", "manifest_files"])) stop("release_shape");
  const receipts = [];
  if (withReceipts) {
    const list = assurance.get("route_receipts");
    if (!Array.isArray(list)) stop("release_shape");
    if (list.length === 0 || list.length > MAX_RECEIPTS) stop("receipt_shape");
    for (const r of list) {
      const done = await checkReceipt(r);
      if (receipts.some((x) => x.completionId === done.completionId)) stop("receipt_shape");
      receipts.push(done);
    }
  }
  const level = assurance.get("binding_level");
  const route = assurance.get("route");
  const notEst = assurance.get("not_established");
  if (!isStr(level) || !isStr(route) || !Array.isArray(notEst)) stop("release_shape");
  if (!["B0", "B1", "B2", "B3"].includes(level)) stop("release_shape");
  const notEstablished = [];
  for (const n of notEst) {
    if (!isStr(n)) stop("release_shape");
    notEstablished.push(n);
  }
  if (!(isStr(commit.get("digest")) && isHex64(commit.get("digest"))) || !isStr(commit.get("scheme")) || !isInt(commit.get("manifest_files"))) stop("release_shape");

  if (now > expires) stop("expired");
  if (expectedRecipient !== null && expectedRecipient !== recipient) stop("wrong_recipient");

  const listed = rel.get("files");
  if (!Array.isArray(listed)) stop("release_shape");
  if (listed.length > MAX_FILES) stop("too_many_files");
  const names = new Set();
  let total = 0;
  for (const f of listed) {
    if (!exactKeys(f, ["name", "sha256", "bytes"])) stop("release_shape");
    const name = f.get("name");
    const digest = f.get("sha256");
    const len = f.get("bytes");
    if (!isStr(name) || !isStr(digest) || !isInt(len)) stop("release_shape");
    if (!validFileName(name)) stop("bad_file_name");
    if (names.has(name)) stop("duplicate_file_name");
    names.add(name);
    if (!isHex64(digest) || len < 0n) stop("release_shape");
    const content = files.get(name);
    if (!isStr(content)) stop("attachment_missing");
    const raw = b64urlDecode(content);
    if (raw === null) stop("bad_attachment");
    total += raw.length;
    if (total > MAX_TOTAL) stop("capsule_too_large");
    if (BigInt(raw.length) !== len || (await sha256Hex(raw)) !== digest) stop("digest_mismatch");
  }
  for (const k of files.keys()) if (!names.has(k)) stop("attachment_extra");

  const releaseDigest = await sha256Hex(release.env.payload);
  let placed = 0;
  for (const c of claims) {
    if (!exactKeys(c, ["type", "version", "release_payload_sha256", "statement", "evidence", "how_to_check"]) || c.get("type") !== "claim" || c.get("version") !== 1n) stop("claim_shape");
    if (c.get("release_payload_sha256") !== releaseDigest) stop("claim_not_bound");
    const ev = c.get("evidence");
    if (!Array.isArray(ev)) stop("claim_shape");
    for (const item of ev) {
      if (!isStr(item)) stop("claim_shape");
      if (!names.has(item)) stop("claim_evidence_missing");
    }
    if (!isStr(c.get("statement")) || !isStr(c.get("how_to_check"))) stop("claim_shape");
    if (named !== null) {
      // the claim at this place must be the one the release names at this place
      const said = serialize(new Map([["evidence", ev], ["how_to_check", c.get("how_to_check")], ["statement", c.get("statement")]]));
      if (named[placed] !== (await sha256Hex(enc.encode(said)))) stop("claims_changed");
    }
    placed += 1;
  }
  // none missing either: a claim taken out leaves the list longer than what is carried
  if (named !== null && named.length !== claims.length) stop("claims_changed");
  if (named === null) notEstablished.push("That no claim was removed from it: this capsule was made before releases named their claims, so a missing claim cannot be seen.");
  return {
    valid: true,
    reason: null,
    signer,
    recipient,
    bindingLevel: level,
    route,
    notEstablished,
    signerPinned: isPinned,
    fileCount: names.size,
    claimCount: claims.length,
    claimsNamed: named !== null,
    claims: claims.map((c) => ({ statement: c.get("statement"), evidence: [...c.get("evidence")], howToCheck: c.get("how_to_check") })),
    receipts,
    created: Number(created),
    expires: Number(expires),
    policyVersion,
    capsuleId,
  };
}

const hexToBytes = (h) => Uint8Array.from(h.match(/../g).map((x) => parseInt(x, 16)));

/** `MODEL:REQUEST:ANSWER` where the last two parts are 64 hex digits each and the model name is not empty. */
function parseSignedText(text) {
  const last = text.lastIndexOf(":");
  if (last < 0) return null;
  const rest = text.slice(0, last);
  const mid = rest.lastIndexOf(":");
  if (mid < 0) return null;
  const model = rest.slice(0, mid);
  const request = rest.slice(mid + 1);
  const response = text.slice(last + 1);
  return model !== "" && model.length <= 200 && isHex64(request) && isHex64(response) ? { model, request, response } : null;
}

/** One model receipt: its shape, that the model signed the text, with a key that is not weak. Same order of checks as the Rust verifier. */
async function checkReceipt(r) {
  if (!exactKeys(r, ["completion_id", "signature_kind", "signer", "signed_text", "signature"])) stop("receipt_shape");
  const id = r.get("completion_id");
  const kind = r.get("signature_kind");
  const signer = r.get("signer");
  const text = r.get("signed_text");
  const sig = r.get("signature");
  if (![id, kind, signer, text, sig].every(isStr)) stop("receipt_shape");
  if (id === "" || id.length > 200 || !/^[\x21-\x7e]+$/.test(id)) stop("receipt_shape");
  if (kind !== "provider_tee") stop("receipt_kind");
  const parsed = parseSignedText(text);
  if (parsed === null || !/^[0-9a-f]{64}$/.test(signer) || !/^[0-9a-f]{128}$/.test(sig)) stop("receipt_shape");
  const keyBytes = hexToBytes(signer);
  if (classifyPublicKey(keyBytes) !== "ok") stop("receipt_key");
  if (!(await verifySignature(keyBytes, enc.encode(text), hexToBytes(sig)))) stop("receipt_signature");
  return { completionId: id, signer, model: parsed.model, requestSha256: parsed.request, responseSha256: parsed.response, signature: sig };
}

function decodeEnvelope(v) {
  if (!isMap(v) || v.size !== 3) return null;
  const payload = v.get("payload");
  const type = v.get("payloadType");
  const sigs = v.get("signatures");
  if (!isStr(payload) || !isStr(type) || !Array.isArray(sigs)) return null;
  const payloadBytes = b64urlDecode(payload);
  if (payloadBytes === null) return null;
  const signatures = [];
  for (const s of sigs) {
    if (!exactKeys(s, ["keyid", "sig"])) return null;
    const keyid = s.get("keyid");
    const sig = s.get("sig");
    if (!isStr(keyid) || !/^ed25519:[0-9a-f]{64}$/.test(keyid) || !isStr(sig)) return null;
    const raw = b64urlDecode(sig);
    if (raw === null || raw.length !== 64) return null;
    signatures.push([keyid, raw]);
  }
  return { type, payload: payloadBytes, signatures };
}

// ---- A share: some files of a release, with a proof and nothing else (see crates/kk-evidence/src/share.rs) ----

const DISCLOSURE_TYPE = "application/vnd.kakurega.disclosure.v1+json";
const SHARE_SCHEME = "salted-merkle-sha256-v1";
const MAX_SHARED = 16;
const MAX_SHARE_BYTES = 16 * 1024 * 1024;

async function sha256(...parts) {
  let n = 0;
  for (const p of parts) n += p.length;
  const all = new Uint8Array(n);
  let at = 0;
  for (const p of parts) {
    all.set(p, at);
    at += p.length;
  }
  return new Uint8Array(await crypto.subtle.digest("SHA-256", all));
}

const unhex32 = (h) => (isStr(h) && isHex64(h) ? Uint8Array.from(h.match(/../g).map((x) => parseInt(x, 16))) : null);
const sameBytes = (a, b) => a.length === b.length && a.every((x, i) => x === b[i]);

/** Whether a leaf at place `m` of a tree of `n` leaves, with this path, gives `want` (RFC 9162, section 2.1.3.2). */
async function pathLeadsTo(leaf, m, n, path, want) {
  if (m >= n) return false;
  let f = m;
  let last = n - 1;
  let r = leaf;
  for (const p of path) {
    if (last === 0) return false;
    if (f % 2 === 1 || f === last) {
      r = await sha256(Uint8Array.of(1), p, r);
      if (f % 2 === 0) {
        while (f % 2 === 0 && f !== 0) {
          f = Math.floor(f / 2);
          last = Math.floor(last / 2);
        }
      }
    } else {
      r = await sha256(Uint8Array.of(1), r, p);
    }
    f = Math.floor(f / 2);
    last = Math.floor(last / 2);
  }
  return last === 0 && sameBytes(r, want);
}

/** Checks a share. `options.pinned` is a list of 32-byte public keys the reader trusts, or null. */
export async function verifyShare(bytes, options = {}) {
  const { pinned = null } = options;
  try {
    return await runShare(bytes, pinned);
  } catch (e) {
    if (e instanceof Stop) return { valid: false, reason: e.reason, signer: null, files: [], totalFiles: 0 };
    throw e;
  }
}

async function runShare(bytes, pinned) {
  if (bytes.length > MAX_SHARE_BYTES) stop("share_too_large");
  const top = parseOrStop(bytes, "share_not_json");
  const canonical = enc.encode(serialize(top));
  if (canonical.length !== bytes.length || !canonical.every((b, i) => b === bytes[i])) stop("share_not_canonical");
  if (!exactKeys(top, ["format", "version", "keys", "envelope", "leaves"]) || top.get("format") !== "kakurega-share") stop("share_shape");
  if (top.get("version") !== 1n) stop("unsupported_version");
  const keys = top.get("keys");
  const leaves = top.get("leaves");
  if (!Array.isArray(keys) || !Array.isArray(leaves)) stop("share_shape");
  const claimed = [];
  for (const k of keys) {
    const raw = isStr(k) ? b64urlDecode(k) : null;
    if (raw === null || raw.length !== 32) stop("bad_key");
    claimed.push(raw);
  }
  const isPinned = pinned !== null;
  const trusted = new Map();
  for (const k of isPinned ? pinned : claimed) {
    if (classifyPublicKey(k) !== "ok") stop("bad_key");
    trusted.set(await keyIdOf(k), k);
  }
  if (trusted.size === 0) stop("no_keys");
  const env = decodeEnvelope(top.get("envelope"));
  if (env === null) stop("bad_envelope");
  if (env.type !== DISCLOSURE_TYPE) stop("unknown_payload_type");
  if (env.signatures.length !== 1) stop("signature_count");
  const [keyid, sig] = env.signatures[0];
  const pub = trusted.get(keyid);
  if (pub === undefined) stop(isPinned ? "untrusted_signer" : "unknown_signer");
  if (!(await verifySignature(pub, pae(env.type, env.payload), sig))) stop("bad_signature");
  let payloadText;
  try {
    payloadText = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(env.payload);
    if (serialize(parseStrict(payloadText)) !== payloadText) stop("payload_not_canonical");
  } catch (e) {
    if (e instanceof Stop) throw e;
    stop("payload_not_canonical");
  }
  // as for a capsule: the key list is not signed, so it must be exactly the key that signed
  if (claimed.length !== 1 || (await keyIdOf(claimed[0])) !== keyid) stop("key_list");
  const st = parseOrStop(payloadText, "payload_not_canonical");
  if (!exactKeys(st, ["type", "version", "capsule_sha256", "release_payload_sha256", "scheme", "root", "leaves"]) || st.get("type") !== "disclosure") stop("disclosure_shape");
  if (st.get("version") !== 1n || st.get("scheme") !== SHARE_SCHEME) stop("unsupported_version");
  const capsuleSha = st.get("capsule_sha256");
  const releaseSha = st.get("release_payload_sha256");
  const want = unhex32(st.get("root"));
  const totalBig = st.get("leaves");
  if (!isStr(capsuleSha) || !isHex64(capsuleSha) || !isStr(releaseSha) || !isHex64(releaseSha) || want === null || !isInt(totalBig)) stop("disclosure_shape");
  if (totalBig < 1n || totalBig > BigInt(MAX_FILES)) stop("disclosure_shape");
  const total = Number(totalBig);
  if (leaves.length === 0 || leaves.length > MAX_SHARED) stop("leaf_count");
  const names = new Set();
  const places = new Set();
  const files = [];
  let lastName = null;
  for (const leaf of leaves) {
    if (!exactKeys(leaf, ["name", "salt", "content", "index", "path"])) stop("leaf_shape");
    const name = leaf.get("name");
    const saltText = leaf.get("salt");
    const contentText = leaf.get("content");
    const index = leaf.get("index");
    const steps = leaf.get("path");
    if (!isStr(name) || !isStr(saltText) || !isStr(contentText) || !isInt(index) || !Array.isArray(steps)) stop("leaf_shape");
    if (!validFileName(name)) stop("bad_file_name");
    // one order only (by the bytes of the name), so the same share cannot be written as two different files
    if (lastName !== null && !(lastName < name)) stop("leaf_order");
    lastName = name;
    if (index < 0n || index >= totalBig || places.has(index) || names.has(name)) stop("leaf_place");
    places.add(index);
    names.add(name);
    const salt = b64urlDecode(saltText);
    const content = b64urlDecode(contentText);
    if (salt === null || salt.length !== 32 || content === null) stop("leaf_shape");
    const hashes = [];
    for (const step of steps) {
      const h = unhex32(step);
      if (h === null) stop("leaf_shape");
      hashes.push(h);
    }
    if (hashes.length > 8) stop("leaf_shape");
    const nameBytes = enc.encode(name);
    const leafHash = await sha256(Uint8Array.of(0), salt, Uint8Array.of(nameBytes.length >> 8, nameBytes.length & 255), nameBytes, content);
    if (!(await pathLeadsTo(leafHash, Number(index), total, hashes, want))) stop("not_in_release");
    files.push({ name, bytes: content });
  }
  const notEstablished = [
    "Anything about the release's other files: this share shows none of their names, sizes or contents.",
    "That what a shared file says is true; only that the signer put exactly this file in the release.",
    "Who the release was made for, or which model route produced it: a share does not carry that.",
  ];
  if (!isPinned) notEstablished.push("Who holds the signing key: no key was pinned, so the key shown comes from the share itself.");
  return {
    valid: true,
    reason: null,
    signer: keyid,
    signerPinned: isPinned,
    capsuleSha256: capsuleSha,
    releasePayloadSha256: releaseSha,
    totalFiles: total,
    files,
    established: [
      `The disclosure statement carries a valid Ed25519 signature from ${keyid}.`,
      `Each of the ${files.length} shared file(s) is exactly one of the ${total} files the signer bound into the release with that statement.`,
      `The capsule it comes from has the SHA-256 digest ${capsuleSha}; a registry record for that digest is about the same release.`,
    ],
    notEstablished,
  };
}
