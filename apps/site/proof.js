// The proof page. Every line it shows is asked of NEAR's public RPC from this tab, now, and compared with what this
// site claims (proof.json, written at build time from the repository). Reads only: no wallet, no key, nothing signed.
import { verifyCapsule } from "@kakurega/verifier";

const $ = (sel) => document.querySelector(sel);
const el = (tag, text, cls) => {
  const n = document.createElement(tag);
  if (text !== undefined) n.textContent = text;
  if (cls) n.className = cls;
  return n;
};
const short = (s) => (s.length > 22 ? s.slice(0, 10) + "…" + s.slice(-8) : s);
const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function base58ToHex(text) {
  let n = 0n;
  for (const c of text) {
    const i = B58.indexOf(c);
    if (i < 0) throw new Error("not base58");
    n = n * 58n + BigInt(i);
  }
  return n.toString(16).padStart(64, "0");
}

let facts;
async function rpc(params) {
  const r = await fetch(facts.rpc, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "query", params: { finality: "final", account_id: facts.registry, ...params } }) });
  if (!r.ok) throw new Error(`the RPC answered HTTP ${r.status}`);
  const body = await r.json();
  if (body.error || body.result?.error) throw new Error(String(body.error?.data ?? body.error?.message ?? body.result.error).slice(0, 200));
  return body.result;
}
async function view(method, args = {}) {
  const result = await rpc({ request_type: "call_function", method_name: method, args_base64: btoa(JSON.stringify(args)) });
  return JSON.parse(new TextDecoder().decode(new Uint8Array(result.result)));
}

/** One check: its row is marked busy, then ok or bad with what NEAR answered. A check never throws past here. */
async function check(id, work) {
  const row = document.querySelector(`[data-check="${id}"]`);
  const out = row.querySelector("[data-out]");
  row.setAttribute("data-state", "busy");
  out.replaceChildren();
  try {
    const { ok, lines } = await work();
    for (const [k, v] of lines) {
      const p = el("p", undefined, "note");
      p.append(el("strong", k + " "), el("span", v, "mono"));
      out.append(p);
    }
    row.setAttribute("data-state", ok ? "ok" : "bad");
    return ok;
  } catch (e) {
    out.append(el("p", "Could not be checked: " + String(e?.message ?? e).slice(0, 200), "note"));
    row.setAttribute("data-state", "warn");
    return null;
  }
}

async function run() {
  const button = $("[data-run]");
  button.disabled = true;
  $("[data-summary]").textContent = "Asking NEAR…";
  let modelKey = null;
  const results = [];
  results.push(
    await check("keys", async () => {
      const r = await rpc({ request_type: "view_access_key_list" });
      return { ok: r.keys.length === 0, lines: [["Access keys on the account:", String(r.keys.length)], ["At block", String(r.block_height)]] };
    }),
  );
  results.push(
    await check("code", async () => {
      const r = await rpc({ request_type: "view_account" });
      const hex = base58ToHex(r.code_hash);
      return { ok: hex === facts.code_sha256, lines: [["NEAR reports", hex], ["This repository builds", facts.code_sha256]] };
    }),
  );
  results.push(
    await check("capsule", async () => {
      if (!globalThis.crypto?.subtle) throw new Error("this browser does not give pages the functions the check needs");
      const bytes = new Uint8Array(await (await fetch("../sample/review.kkg")).arrayBuffer());
      const r = await verifyCapsule(bytes, { now: Math.floor(Date.now() / 1000) });
      const signers = [...new Set((r.receipts ?? []).map((x) => x.signer))];
      if (r.valid && signers.length === 1) modelKey = signers[0];
      return { ok: r.valid && signers.length === 1, lines: [["Capsule", r.valid ? "valid, every signature checks out" : "not valid: " + r.reason], ["Answers signed by the model", String((r.receipts ?? []).length)], ["Model's key", modelKey ?? "none"]] };
    }),
  );
  results.push(
    await check("hardware", async () => {
      if (!modelKey) throw new Error("the capsule gave no model key to ask about");
      const [h, verifier] = [await view("hardware", { signer: modelKey }), await view("verifier")];
      if (!h) return { ok: false, lines: [["NEAR's record for this key:", "none"]] };
      return {
        ok: h.signer === modelKey && h.tcb_status === "UpToDate" && verifier === facts.verifier,
        lines: [["Checked by NEAR's verifier contract", verifier], ["Intel patch level", h.tcb_status + (h.advisory_ids.length ? " with advisories " + h.advisory_ids.join(", ") : ", no advisories")], ["Measurement of the machine (MRTD)", short(h.mr_td)], ["Recorded", new Date(Number(BigInt(h.attested_at) / 1000000n)).toISOString().replace("T", " ").slice(0, 19) + " UTC"]],
      };
    }),
  );
  results.push(
    await check("order", async () => {
      const c = facts.registered_capsule;
      const entry = await view("get", { capsule_sha256: c.capsule_sha256, capsule_signer: c.capsule_signer });
      if (!entry) return { ok: false, lines: [["NEAR's record for this capsule:", "none"]] };
      const order = entry.order ? await view("order", { order_id: entry.order }) : null;
      const ok = entry.receipt_count === c.receipt_count && entry.order === c.order && order?.state === "Paid" && order.require_hardware === true && order.capsule_sha256 === c.capsule_sha256;
      return {
        ok,
        lines: [["Capsule", short(c.capsule_sha256)], ["Model signatures the contract verified", String(entry.receipt_count)], ["Registered by", entry.registered_by], ["Order " + entry.order, order ? `${order.state}; required a hardware record: ${order.require_hardware ? "yes" : "no"}; ${Number(BigInt(order.amount) / 10n ** 21n) / 1000} NEAR (testnet)` : "none"]],
      };
    }),
  );
  results.push(
    await check("counts", async () => {
      const [total, orders, fee] = [await view("total"), await view("orders_total"), await view("fee_terms")];
      return { ok: fee.bps === facts.fee_bps, lines: [["Capsules registered", String(total)], ["Orders opened", String(orders)], ["Fee, fixed in the code", `${fee.bps / 100}% to ${fee.account}`]] };
    }),
  );
  const passed = results.filter((x) => x === true).length;
  const failed = results.filter((x) => x === false).length;
  const summary = $("[data-summary]");
  summary.textContent = failed ? `${failed} of ${results.length} checks did not match. That is a real finding: tell us.` : passed === results.length ? `All ${results.length} checks match, asked of NEAR just now from your browser.` : `${passed} of ${results.length} checks match; ${results.length - passed - failed} could not be asked (the public RPC may be busy). Try again.`;
  summary.className = failed ? "bad" : passed === results.length ? "ok" : "note";
  button.disabled = false;
  button.textContent = "Ask NEAR again";
}

(async () => {
  try {
    facts = await (await fetch("../proof.json")).json();
  } catch {
    $("[data-summary]").textContent = "This page's list of claims could not be loaded.";
    return;
  }
  for (const n of document.querySelectorAll("[data-registry]")) n.textContent = facts.registry;
  $("[data-run]").addEventListener("click", () => void run());
  void run();
})();
