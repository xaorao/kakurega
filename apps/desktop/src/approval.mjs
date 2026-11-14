import { deadlineLines, timeLeft } from "./expiry.mjs";
import { english } from "./strings.mjs";

export function describe(a, t = english) {
  const files = (a.files ?? []).map((f) => t("desc.file", { name: f.name, bytes: f.bytes, sha: String(f.sha256 ?? "").slice(0, 16) }));
  const removed = (a.removed ?? []).map((r) => `  ${r.what} (${r.why})`);
  const notEstablished = (a.not_established ?? []).map((n) => `  - ${n}`);
  const d = a.disclosure;
  const disclosure = d
    ? [t(d.releases_before === 1 ? "desc.disclosure.one" : "desc.disclosure", { after: d.files_after, total: d.project_files, before: d.releases_before }), ...(d.reasons ?? []).map((r) => `  ! ${r}`), t("desc.notChecked", { list: (d.not_checked ?? []).join(" ") }), ""]
    : [];
  return [
    t("desc.recipient", { recipient: a.recipient }),
    t("desc.digest", { digest: a.digest }),
    ...deadlineLines(a, t),
    "",
    t("desc.files"),
    ...files,
    "",
    ...disclosure,
    t("desc.withheld"),
    ...(removed.length ? removed : [t("desc.none")]),
    "",
    t("desc.notEstablished"),
    ...(notEstablished.length ? notEstablished : [t("desc.nothingListed")]),
  ].join("\n");
}

export function createApprover({ client, confirm, authenticate, sign = null, seen = new Set(), t = english }) {
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
      const yes = a.requires_proof && sign?.ownsDialog ? true : await confirm(describe(a, t), a);
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
        const proven = await authenticate(t("approve.reason", { digest: String(a.digest).slice(0, 12), recipient: a.recipient }));
        // "unavailable": this computer has no Touch ID. The owner's answer in the native window stands by itself then, as
        // a digest typed at a terminal does. A prompt that was shown and cancelled or failed still refuses.
        if (proven !== true && proven !== "unavailable") {
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
