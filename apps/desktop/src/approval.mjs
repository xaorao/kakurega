export function describe(a) {
  const files = (a.files ?? []).map((f) => `  ${f.name}  ${f.bytes} bytes  sha256 ${String(f.sha256 ?? "").slice(0, 16)}`);
  const removed = (a.removed ?? []).map((r) => `  ${r.what} (${r.why})`);
  const notEstablished = (a.not_established ?? []).map((n) => `  - ${n}`);
  return [
    `Recipient: ${a.recipient}`,
    `Digest: ${a.digest}`,
    "",
    "These exact files will be signed and handed over, nothing else:",
    ...files,
    "",
    "Withheld and never released:",
    ...(removed.length ? removed : ["  (none)"]),
    "",
    "This release does not establish:",
    ...(notEstablished.length ? notEstablished : ["  (nothing listed)"]),
  ].join("\n");
}

export function createApprover({ client, confirm, authenticate, seen = new Set() }) {
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
      const yes = await confirm(describe(a), a);
      if (!yes) {
        await client.post(`/admin/approvals/${item.id}/deny`);
        done.push({ id: item.id, outcome: "denied" });
        continue;
      }
      const proven = await authenticate(`Approve release ${String(a.digest).slice(0, 12)} to ${a.recipient}`);
      if (!proven) {
        await client.post(`/admin/approvals/${item.id}/deny`);
        done.push({ id: item.id, outcome: "authentication_failed" });
        continue;
      }
      const r = await client.post(`/admin/approvals/${item.id}/approve`, { ack: String(a.digest).slice(0, 12) });
      done.push({ id: item.id, outcome: r.status === 200 ? "approved" : "refused" });
    }
    return done;
  };
}
