import { deadlineLines, timeLeft } from "./expiry.mjs";

export function describe(a) {
  const files = (a.files ?? []).map((f) => `  ${f.name}  ${f.bytes} bytes  sha256 ${String(f.sha256 ?? "").slice(0, 16)}`);
  const removed = (a.removed ?? []).map((r) => `  ${r.what} (${r.why})`);
  const notEstablished = (a.not_established ?? []).map((n) => `  - ${n}`);
  const d = a.disclosure;
  const disclosure = d ? [`Cumulative disclosure: ${d.files_after} of ${d.project_files} project files would have been shown to this recipient (${d.releases_before} earlier release(s)).`, ...(d.reasons ?? []).map((r) => `  ! ${r}`), `  Not checked: ${(d.not_checked ?? []).join(" ")}`, ""] : [];
  return [
    `Recipient: ${a.recipient}`,
    `Digest: ${a.digest}`,
    ...deadlineLines(a),
    "",
    "These exact files will be signed and handed over, nothing else:",
    ...files,
    "",
    ...disclosure,
    "Withheld and never released:",
    ...(removed.length ? removed : ["  (none)"]),
    "",
    "This release does not establish:",
    ...(notEstablished.length ? notEstablished : ["  (nothing listed)"]),
  ].join("\n");
}

export function createApprover({ client, confirm, authenticate, sign = null, seen = new Set() }) {
  return async function pollOnce() {
    const list = await client.get("/admin/approvals");
    if (list.status !== 200) return [];
    const done = [];
    for (const item of list.json?.approvals ?? []) {
      if (typeof item.id !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(item.id) || seen.has(item.id)) continue;
      seen.add(item.id);
      const detail = await client.get(`/admin/approvals/${item.id}`);
      if (detail.status !== 200) continue;
      const a = detail.json;
      if (a.approver_missing === true) {
        // this installation only takes signed approvals and no key is enrolled: asking the owner would end in a refusal, so say so instead
        done.push({ id: item.id, outcome: "needs_enrolled_key" });
        continue;
      }
      if (timeLeft(a.expires, a.server_time).state === "expired") {
        // nothing to decide: the service would refuse it, so no window is shown and the request leaves the waiting list
        await client.post(`/admin/approvals/${item.id}/deny`);
        done.push({ id: item.id, outcome: "expired" });
        continue;
      }
      const yes = a.requires_proof && sign?.ownsDialog ? true : await confirm(describe(a), a);
      if (!yes) {
        await client.post(`/admin/approvals/${item.id}/deny`);
        done.push({ id: item.id, outcome: "denied" });
        continue;
      }
      const body = { ack: String(a.digest).slice(0, 12) };
      if (a.requires_proof) {
        const proof = sign ? await sign(item.id).catch(() => null) : null;
        if (typeof proof !== "string" || !proof) {
          await client.post(`/admin/approvals/${item.id}/deny`);
          done.push({ id: item.id, outcome: "signing_failed" });
          continue;
        }
        body.proof = proof;
      } else {
        const proven = await authenticate(`Approve release ${String(a.digest).slice(0, 12)} to ${a.recipient}`);
        if (!proven) {
          await client.post(`/admin/approvals/${item.id}/deny`);
          done.push({ id: item.id, outcome: "authentication_failed" });
          continue;
        }
      }
      const r = await client.post(`/admin/approvals/${item.id}/approve`, body);
      done.push({ id: item.id, outcome: r.status === 200 ? "approved" : "refused" });
    }
    return done;
  };
}
