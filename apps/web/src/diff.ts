export type DiffLine = {
  kind: "add" | "del" | "ctx" | "hunk" | "file" | "meta";
  text: string;
  old?: number;
  now?: number;
  /** Character ranges [from, to) of the part of this line that differs from the line it replaces. */
  marks?: [number, number][];
};

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

/** A line longer than this is not compared word by word. */
const MARK_MAX_CHARS = 5000;
/** When more than this share of a line differs, the whole line is the change and nothing is marked inside it. */
const MARK_MAX_SHARE = 0.6;

type Token = { from: number; to: number; text: string };

/** Words (letters, digits and marks of any script, and underscores), runs of blanks, runs of operator characters (== != <= ->), and single other characters. */
function tokens(text: string): Token[] {
  const out: Token[] = [];
  for (const m of text.matchAll(/[\p{L}\p{N}\p{M}_]+|[ \t]+|[=!<>+\-*\/%&|^~]+|[^\p{L}\p{N}\p{M}_ \t]/gu)) {
    out.push({ from: m.index ?? 0, to: (m.index ?? 0) + m[0].length, text: m[0] });
  }
  return out;
}

/** The part of `a` and of `b` between their common beginning and common end, as character ranges (empty side = null). */
function differing(a: string, b: string): { a: [number, number] | null; b: [number, number] | null } | null {
  if (a === b || a.length > MARK_MAX_CHARS || b.length > MARK_MAX_CHARS) return null;
  const ta = tokens(a);
  const tb = tokens(b);
  const most = Math.min(ta.length, tb.length);
  let head = 0;
  while (head < most && ta[head]!.text === tb[head]!.text) head++;
  let tail = 0;
  while (tail < most - head && ta[ta.length - 1 - tail]!.text === tb[tb.length - 1 - tail]!.text) tail++;
  const range = (t: Token[], len: number): [number, number] | null => {
    const first = t[head];
    const last = t[len - tail - 1];
    return first && last && len - tail > head ? [first.from, last.to] : null;
  };
  const ra = range(ta, ta.length);
  const rb = range(tb, tb.length);
  if (!ra && !rb) return null;
  const share = (r: [number, number] | null, text: string) => (r ? (r[1] - r[0]) / Math.max(1, text.length) : 0);
  if (share(ra, a) > MARK_MAX_SHARE || share(rb, b) > MARK_MAX_SHARE) return null;
  return { a: ra, b: rb };
}

/**
 * Marks, inside each removed line and the added line that replaces it, the words that differ. Only a run of removed lines
 * followed directly by the same number of added lines is compared, one for one: anything else is not a replacement of a
 * line by a line, and marking it would guess. The text, the numbers and the kinds are never changed.
 */
export function markChanges(lines: DiffLine[]): DiffLine[] {
  const out = lines.map((l) => ({ ...l }));
  let i = 0;
  while (i < out.length) {
    if (out[i]!.kind !== "del") {
      i++;
      continue;
    }
    let d = i;
    while (d < out.length && out[d]!.kind === "del") d++;
    let a = d;
    while (a < out.length && out[a]!.kind === "add") a++;
    if (d - i === a - d) {
      for (let k = 0; k < d - i; k++) {
        const del = out[i + k]!;
        const add = out[d + k]!;
        const diff = differing(del.text, add.text);
        if (!diff) continue;
        if (diff.a) del.marks = [diff.a];
        if (diff.b) add.marks = [diff.b];
      }
    }
    i = Math.max(a, i + 1);
  }
  return out;
}
