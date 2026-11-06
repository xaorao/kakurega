// Find in the open conversation: where a typed text occurs in the messages, without changing the page's own markup. The
// page colours the matches with the browser's highlight feature where it has one; where it has not, the count and the
// jump to each match still work.

export interface Match {
  /** Which message, counted from 0. */
  turn: number;
  start: number;
  end: number;
}

/** The longest text searched for, and the most matches kept: a pasted page must not freeze the screen. */
export const MAX_QUERY = 200;
export const MAX_MATCHES = 500;

/** Every place `query` occurs in `texts`, in order, ignoring case. An empty or blank query finds nothing. */
export function findMatches(texts: readonly string[], query: string): Match[] {
  const q = query.trim().slice(0, MAX_QUERY).toLowerCase();
  if (q === "") return [];
  const out: Match[] = [];
  for (let turn = 0; turn < texts.length; turn++) {
    const text = texts[turn] ?? "";
    const hay = text.toLowerCase();
    // lower-casing can change a text's length (some letters grow), so offsets are only valid when it did not
    if (hay.length !== text.length) continue;
    let at = hay.indexOf(q);
    while (at !== -1) {
      out.push({ turn, start: at, end: at + q.length });
      if (out.length >= MAX_MATCHES) return out;
      at = hay.indexOf(q, at + q.length);
    }
  }
  return out;
}

/** The match after (`step` 1) or before (-1) the current one, wrapping round; null when there are none. */
export function stepTo(count: number, current: number | null, step: 1 | -1): number | null {
  if (count <= 0) return null;
  if (current === null) return step === 1 ? 0 : count - 1;
  return (current + step + count) % count;
}
