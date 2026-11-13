import { b64urlDecode, verifyCapsule, verifyShare } from "@kakurega/verifier";
import { NETWORKS, amount, hex, isAccount, lookup, sha256Hex, when } from "./chain.js";

const $ = (id) => document.getElementById(id);
const out = $("out");

const REASONS = {
  capsule_not_json: "The file is not a capsule.",
  capsule_not_canonical: "The file was changed after it was signed (its bytes are not in the one allowed form).",
  capsule_shape: "The file does not have the shape of a capsule.",
  unsupported_version: "This capsule is from a version this page does not know.",
  bad_key: "A key in the capsule is not usable.",
  key_list: "The capsule's key list is not exactly the one key that signed it: the file was altered after signing.",
  no_keys: "The capsule carries no key.",
  signature_count: "An envelope does not have exactly one signature.",
  bad_signature: "A signature does not match its content.",
  receipt_signature: "A model receipt's signature does not match: the receipt was altered or forged.",
  receipt_key: "A model receipt names a key that cannot be trusted.",
  receipt_kind: "A model receipt is not a hardware-enclave signature.",
  receipt_shape: "A model receipt is malformed, repeated, or there are too many.",
  expired: "The capsule has expired.",
  share_not_json: "The file is not a share.",
  share_not_canonical: "The file was changed after it was made (its bytes are not in the one allowed form).",
  share_shape: "The file does not have the shape of a share.",
  share_too_large: "The file is too large to be a share.",
  not_in_release: "A file in this share is not part of the signed release: it was changed, or was never in it.",
  leaf_shape: "A file entry in this share is malformed.",
  leaf_place: "A file entry in this share claims a place it cannot have.",
  leaf_order: "The file entries of this share are not in their one allowed order, or one is repeated.",
  leaf_count: "This share carries no file, or too many.",
  disclosure_shape: "The signed statement in this share does not have the shape it must have.",
  bad_file_name: "A file in this share has a name that is not a plain file name.",
  untrusted_signer: "It was not signed by the key you pasted.",
  unknown_signer: "It was signed by a key other than the one it carries.",
  capsule_too_large: "The file is too large to be a capsule.",
};

function el(tag, text, cls) {
  const e = document.createElement(tag);
  if (text !== undefined) e.textContent = text;
  if (cls) e.className = cls;
  return e;
}

function list(title, items) {
  const wrap = document.createDocumentFragment();
  wrap.append(el("h2", title));
  const ul = el("ul");
  for (const i of items) ul.append(el("li", i));
  wrap.append(ul);
  return wrap;
}

/** Text that came from the chain: no control characters, and never longer than a line or two. */
function plain(v) {
  return String(v ?? "?").replace(/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/g, "").slice(0, 300);
}

function hardwareItems(h) {
  const items = [
    `NEAR's verifier checked its Intel TDX quote against Intel's collateral on ${when(h.attested_at)}`,
    `Intel patch level then: ${plain(h.tcb_status)}`,
    `Asked for by: ${plain(h.attested_by)}`,
  ];
  if (h.compose_sha256) items.push(`Deployment description (SHA-256): ${plain(h.compose_sha256)}`);
  return items;
}

function showChain(box, place, found) {
  box.replaceChildren();
  const { entry, hardware, order, count, unregistered, different } = found;
  if (!entry) {
    const v = el("div", "Not registered on NEAR under this signing key", "verdict");
    v.setAttribute("role", "status");
    box.append(v, el("p", `Looked up at ${place}. That says only that nobody registered this exact file there.`, "note"));
  } else {
    const v = el("div", "Registered on NEAR", "verdict ok");
    v.setAttribute("role", "status");
    box.append(v);
    const dl = el("dl");
    const row = (k, val, code) => dl.append(el("dt", k), el("dd", val, code ? "code" : ""));
    row("Registered by", plain(entry.registered_by), true);
    row("When", when(entry.registered_at));
    row("Where", place, true);
    const n = count;
    box.append(dl);
    box.append(el("p", "That account proved to the contract that it controls the capsule's signing key."));
    box.append(el("p", n === 0 ? "No model signatures were registered with it." : n === 1 ? "The contract verified 1 model signature registered with it." : `The contract verified ${n} model signatures registered with it.`));
    if (different) {
      box.append(el("p", "The model signatures registered with it are not this capsule's own: their digest differs.", "bad"));
    } else if (unregistered > 0) {
      box.append(el("p", `${unregistered} of this capsule's model signatures were not registered with it.`, "note"));
    }
  }
  for (const h of hardware) {
    if (h.record) box.append(list(`Hardware record for model key ${h.signer.slice(0, 16)}…`, hardwareItems(h.record)));
    else box.append(el("p", `No hardware record at this registry for model key ${h.signer.slice(0, 16)}….`, "note"));
  }
  if (order && order.order) {
    const o = order.order;
    box.append(
      list(`Registered for order ${plain(order.id)}`, [
        `State: ${plain(o.state)}`,
        `Amount: ${plain(amount(o))} to the reviewer`,
        ...(o.fee && o.fee !== "0" ? [`Fee on top: ${plain(amount({ ...o, amount: o.fee }))}, to the registry's fee account`] : []),
        `Client: ${plain(o.client)}`,
        `Reviewer: ${plain(o.reviewer)}`,
      ]),
    );
  }
  const not = ["That any claim in the capsule is true."];
  if (hardware.some((h) => h.record)) {
    not.push(
      "That a hardware quote is recent, beyond Intel's collateral accepting it at that time.",
      "Which software ran: the measurements are recorded, none is approved by the contract.",
      "Anything about the GPU.",
    );
  }
  box.append(list("What the chain does not establish", not));
}

/** The part of the page that asks NEAR, shown under a valid capsule. Nothing is sent until the button is pressed. */
function chainSection(bytes, report) {
  const wrap = el("section");
  wrap.id = "chain";
  wrap.append(el("h2", "On NEAR (optional)"));
  wrap.append(
    el(
      "p",
      "Pressing the button sends this capsule's digest and the keys named in it to a public NEAR server, to ask what the Kakurega registry holds about it. Nothing else leaves this page.",
      "note",
    ),
  );
  const net = document.createElement("select");
  net.id = "net";
  for (const name of Object.keys(NETWORKS)) {
    const o = el("option", name);
    o.value = name;
    net.append(o);
  }
  const reg = document.createElement("input");
  reg.type = "text";
  reg.id = "registry";
  reg.autocomplete = "off";
  reg.spellcheck = false;
  reg.value = NETWORKS.testnet.registry;
  net.addEventListener("change", () => {
    reg.value = NETWORKS[net.value].registry;
  });
  const netLabel = el("label", "Network");
  netLabel.htmlFor = "net";
  const regLabel = el("label", "Registry account");
  regLabel.htmlFor = "registry";
  const go = el("button", "Look this capsule up on NEAR");
  go.type = "button";
  go.id = "lookup";
  const box = el("div");
  box.id = "chain-out";
  box.setAttribute("aria-live", "polite");
  go.addEventListener("click", async () => {
    const registry = reg.value.trim();
    const fail = (m) => {
      const a = el("p", m, "bad");
      a.setAttribute("role", "alert");
      box.replaceChildren(a);
    };
    if (!isAccount(registry)) return fail("That is not a NEAR account name.");
    let signer = null;
    try {
      const k = JSON.parse(new TextDecoder().decode(bytes)).keys;
      const raw = Array.isArray(k) && k.length === 1 ? b64urlDecode(k[0]) : null;
      if (raw && raw.length === 32) signer = hex(raw);
    } catch {
      signer = null;
    }
    if (!signer) return fail("This capsule's signing key could not be read.");
    go.disabled = true;
    box.replaceChildren(el("p", "Asking NEAR…", "note"));
    try {
      const digest = report.capsuleSha256 || (await sha256Hex(bytes));
      const found = await lookup(NETWORKS[net.value], registry, digest, signer, report.receipts || []);
      showChain(box, `${registry} on NEAR ${net.value}`, found);
    } catch (e) {
      fail(e && e.message ? e.message : "The lookup could not be done.");
    } finally {
      go.disabled = false;
    }
  });
  wrap.append(netLabel, net, regLabel, reg, go, box);
  return wrap;
}

/** A shared file's text when it is plain text of a readable size; otherwise null and only its size is shown. */
function readable(bytes) {
  if (bytes.length > 20000) return null;
  try {
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(text) ? null : text;
  } catch {
    return null;
  }
}

function showShare(r, bytes) {
  out.replaceChildren();
  const v = el("div", r.valid ? "Valid share: these files are part of the signed release" : "Not valid", "verdict " + (r.valid ? "ok" : "bad"));
  v.setAttribute("role", "status");
  out.append(v);
  if (!r.valid) {
    out.append(el("p", REASONS[r.reason] || `Refused (${r.reason}).`));
    out.append(el("p", `Code: ${r.reason}`, "note"));
    return;
  }
  const dl = el("dl");
  const row = (k, val, code) => {
    dl.append(el("dt", k), el("dd", String(val), code ? "code" : ""));
  };
  row("Signer", r.signer, true);
  row("Signer pinned", r.signerPinned ? "yes, matches the key you pasted" : "no, this is only the key the share carries");
  row("Shared", `${r.files.length} of the release's ${r.totalFiles} files`);
  row("Capsule digest", r.capsuleSha256, true);
  out.append(dl);
  out.append(el("h2", "The shared files"));
  for (const f of r.files) {
    const box = el("section");
    box.className = "shared";
    box.append(el("h3", `${f.name} (${f.bytes.length} bytes)`));
    const text = readable(f.bytes);
    // shown as text only: nothing in a shared file is ever treated as markup
    box.append(text === null ? el("p", "Not shown here: it is not plain text, or it is long.", "note") : el("pre", text));
    out.append(box);
  }
  out.append(list("What this check establishes", r.established));
  out.append(list("What this check does not establish", r.notEstablished));
  out.append(chainSection(bytes, r));
}

function show(r, bytes) {
  out.replaceChildren();
  const v = el("div", r.valid ? "Valid: the signatures check out" : "Not valid", "verdict " + (r.valid ? "ok" : "bad"));
  v.setAttribute("role", "status");
  out.append(v);
  if (!r.valid) {
    out.append(el("p", REASONS[r.reason] || `Refused (${r.reason}).`));
    out.append(el("p", `Code: ${r.reason}`, "note"));
    return;
  }
  const dl = el("dl");
  const row = (k, val, code) => {
    dl.append(el("dt", k), el("dd", String(val), code ? "code" : ""));
  };
  row("Signer", r.signer, true);
  row("Signer pinned", r.signerPinned ? "yes, matches the key you pasted" : "no, this is only the key the capsule carries");
  if (r.recipient) row("Recipient", r.recipient);
  row("Files", r.fileCount);
  row("Claims", r.claimCount);
  if (r.created) row("Created", new Date(r.created * 1000).toISOString().replace("T", " ").slice(0, 19) + " UTC");
  if (r.expires) row("Expires", new Date(r.expires * 1000).toISOString().replace("T", " ").slice(0, 19) + " UTC");
  row("Capsule id", r.capsuleId, true);
  out.append(dl);
  const rs = r.receipts || [];
  if (rs.length) {
    out.append(
      list(
        `Model signatures checked (${rs.length})`,
        rs.map((x) => `${x.model}: signature by ${x.signer.slice(0, 16)}… over request ${x.requestSha256.slice(0, 12)}… and answer ${x.responseSha256.slice(0, 12)}…`),
      ),
    );
  } else {
    out.append(el("p", "This capsule carries no model signatures.", "note"));
  }
  const not = [...(r.notEstablished || [])];
  if (rs.length) {
    not.push(
      "That a model signer key belongs to attested confidential hardware (this page cannot check a hardware quote).",
      "That a model's answer is true; only that the model's key signed exactly that request and answer.",
    );
  }
  if (not.length) out.append(list("What this check does not establish", not));
  out.append(chainSection(bytes, r));
}

let last = null;

async function check(file) {
  last = file;
  const pin = $("pin").value.trim();
  let pinned = null;
  if (pin) {
    const raw = b64urlDecode(pin);
    if (!raw || raw.length !== 32) {
      out.replaceChildren(el("div", "The key you pasted is not 43 base64url characters.", "verdict bad"));
      return;
    }
    pinned = [raw];
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  try {
    // a share says what it is; the check that follows decides whether it is a valid one
    let kind = null;
    try {
      kind = JSON.parse(new TextDecoder().decode(bytes)).format;
    } catch {
      kind = null;
    }
    if (kind === "kakurega-share") showShare(await verifyShare(bytes, { pinned }), bytes);
    else show(await verifyCapsule(bytes, { pinned, now: Math.floor(Date.now() / 1000) }), bytes);
  } catch (e) {
    // a browser that does not give pages Web Crypto cannot run the check at all; that is not the file's fault
    out.replaceChildren(el("div", globalThis.crypto?.subtle ? "The check could not run on this file." : "This browser does not give pages the Web Crypto functions the check needs. Open this page in a current browser, or run: kaku verify FILE", "verdict bad"));
  }
}

const drop = $("drop");
const input = $("file");
drop.addEventListener("click", () => input.click());
drop.addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    input.click();
  }
});
input.addEventListener("change", () => {
  const f = input.files[0];
  input.value = "";
  if (f) check(f);
});
$("pin").addEventListener("input", () => last && check(last));
drop.addEventListener("dragover", (e) => {
  e.preventDefault();
  drop.classList.add("over");
});
drop.addEventListener("dragleave", () => drop.classList.remove("over"));
drop.addEventListener("drop", (e) => {
  e.preventDefault();
  drop.classList.remove("over");
  const f = e.dataTransfer.files[0];
  if (f) check(f);
});
