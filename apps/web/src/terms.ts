import { visible } from "./visible.ts";

export interface Piece {
  text: string;
  hit: boolean;
}

const MAX_TERMS = 12;
const MAX_TERM_LENGTH = 80;

/**
 * Cuts `text` into pieces so the words a search matched can be shown marked. Matching ignores case, and every piece keeps the
 * note's own spelling. The terms are literal text, never a pattern, and the pieces are plain strings, so nothing in a note or
 * a search can become markup or change what the pattern matches. A term that is empty or longer than 80 characters is skipped,
 * and no more than 12 are used.
 */
export function markTerms(raw: string, terms: readonly string[]): Piece[] {
  const text = visible(raw);
  const usable = [...new Set(terms.map((t) => t.trim()).filter((t) => t.length > 0 && t.length <= MAX_TERM_LENGTH))]
    .slice(0, MAX_TERMS)
    .sort((a, b) => b.length - a.length);
  if (usable.length === 0 || text.length === 0) return [{ text, hit: false }];
  const pattern = new RegExp(usable.map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|"), "giu");
  const pieces: Piece[] = [];
  let at = 0;
  for (const m of text.matchAll(pattern)) {
    const start = m.index ?? 0;
    if (m[0].length === 0) continue;
    if (start > at) pieces.push({ text: text.slice(at, start), hit: false });
    const last = pieces[pieces.length - 1];
    if (last?.hit && at === start) last.text += m[0];
    else pieces.push({ text: m[0], hit: true });
    at = start + m[0].length;
  }
  if (at < text.length) pieces.push({ text: text.slice(at), hit: false });
  return pieces.length > 0 ? pieces : [{ text, hit: false }];
}
