export type DiffLine = { kind: "add" | "del" | "ctx" | "hunk" | "file"; text: string; old?: number; now?: number };

/** Reads a unified diff into lines with their old and new line numbers. Anything unexpected is shown as plain context. */
export function parseDiff(diff: string): DiffLine[] {
  const out: DiffLine[] = [];
  let o = 0;
  let n = 0;
  for (const raw of diff.split("\n")) {
    if (raw === "" && out.length && out[out.length - 1]?.kind !== "hunk") continue;
    if (raw.startsWith("--- ") || raw.startsWith("+++ ")) out.push({ kind: "file", text: raw });
    else if (raw.startsWith("@@")) {
      const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)/.exec(raw);
      o = m ? Number(m[1]) : 0;
      n = m ? Number(m[2]) : 0;
      out.push({ kind: "hunk", text: raw });
    } else if (raw.startsWith("+")) out.push({ kind: "add", text: raw.slice(1), now: n++ });
    else if (raw.startsWith("-")) out.push({ kind: "del", text: raw.slice(1), old: o++ });
    else out.push({ kind: "ctx", text: raw.startsWith(" ") ? raw.slice(1) : raw, old: o++, now: n++ });
  }
  return out;
}

