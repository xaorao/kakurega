export type DiffLine = { kind: "add" | "del" | "ctx" | "hunk" | "file" | "meta"; text: string; old?: number; now?: number };

/**
 * Reads a unified diff into lines with their old and new line numbers. Inside a hunk the header says how many old and new
 * lines to expect, so a removed line that reads "-- note" (shown as "--- note") or an added "++ x" is a changed line, not a
 * file header. Anything unexpected is shown as plain context.
 */
export function parseDiff(diff: string): DiffLine[] {
  const out: DiffLine[] = [];
  let o = 0;
  let n = 0;
  let oldLeft = 0;
  let newLeft = 0;
  for (const raw of diff.split("\n")) {
    const inHunk = oldLeft > 0 || newLeft > 0;
    if (raw.startsWith("\\") && out.length) {
      out.push({ kind: "meta", text: raw }); // "\ No newline at end of file" describes the line above; it is not a line
      continue;
    }
    if (inHunk && raw.startsWith("+")) {
      out.push({ kind: "add", text: raw.slice(1), now: n++ });
      newLeft--;
      continue;
    }
    if (inHunk && raw.startsWith("-")) {
      out.push({ kind: "del", text: raw.slice(1), old: o++ });
      oldLeft--;
      continue;
    }
    if (inHunk && (raw.startsWith(" ") || raw === "")) {
      out.push({ kind: "ctx", text: raw.slice(1), old: o++, now: n++ });
      oldLeft--;
      newLeft--;
      continue;
    }
    if (raw === "" && out.length && out[out.length - 1]?.kind !== "hunk") continue;
    if (raw.startsWith("--- ") || raw.startsWith("+++ ")) out.push({ kind: "file", text: raw });
    else if (raw.startsWith("@@")) {
      const m = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))?/.exec(raw);
      o = m ? Number(m[1]) : 0;
      n = m ? Number(m[3]) : 0;
      oldLeft = m ? (m[2] === undefined ? 1 : Number(m[2])) : 0;
      newLeft = m ? (m[4] === undefined ? 1 : Number(m[4])) : 0;
      out.push({ kind: "hunk", text: raw });
    } else if (raw.startsWith("+")) out.push({ kind: "add", text: raw.slice(1), now: n++ });
    else if (raw.startsWith("-")) out.push({ kind: "del", text: raw.slice(1), old: o++ });
    else out.push({ kind: "ctx", text: raw.startsWith(" ") ? raw.slice(1) : raw, old: o++, now: n++ });
  }
  return out;
}
