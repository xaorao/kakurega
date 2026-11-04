export interface Matchable {
  label: string;
  group: string;
}

/** How well one typed word fits a text: whole-word start 100, any word start 80, anywhere inside 60, letters in order 30 minus gaps, else -1. */
export function wordScore(text: string, word: string): number {
  const t = text.toLowerCase();
  const at = t.indexOf(word);
  if (at === 0) return 100;
  if (at > 0) return /[^\p{L}\p{N}]/u.test(t[at - 1] ?? "") ? 80 : 60;
  const chars = [...word];
  const hay = [...t];
  let from = 0;
  let gaps = 0;
  let last = -1;
  for (const c of chars) {
    const i = hay.indexOf(c, from);
    if (i < 0) return -1;
    if (last >= 0) gaps += i - last - 1;
    last = i;
    from = i + 1;
  }
  return Math.max(1, 30 - gaps);
}

/** Items that fit every typed word, best first (a match in the label counts double against one in the group). Ties keep the given order; no query keeps it as is. */
export function matchItems<T extends Matchable>(items: T[], query: string): T[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return items;
  const scored: { item: T; score: number; index: number }[] = [];
  items.forEach((item, index) => {
    let score = 0;
    for (const w of words) {
      const s = Math.max(2 * wordScore(item.label, w), wordScore(item.group, w));
      if (s < 0) return;
      score += s;
    }
    scored.push({ item, score, index });
  });
  scored.sort((a, b) => b.score - a.score || a.index - b.index);
  return scored.map((s) => s.item);
}
