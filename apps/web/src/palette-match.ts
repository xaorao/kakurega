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

/** Rows move by this many on Page Up and Page Down. */
export const PAGE = 10;

/** Positions where a group header is shown. Only while browsing: a search result carries its group on the row instead. */
export function sectionHeads<T extends Matchable>(shown: T[], query: string): number[] {
  if (query.trim() !== "") return [];
  const heads: number[] = [];
  shown.forEach((item, i) => {
    if (i === 0 || shown[i - 1]?.group !== item.group) heads.push(i);
  });
  return heads;
}

export type Move = "up" | "down" | "pageUp" | "pageDown";

/** The next highlighted position among `n` rows. Arrow keys wrap around; page moves stop at the ends. */
export function moveSelection(sel: number, n: number, move: Move): number {
  if (n <= 0) return 0;
  switch (move) {
    case "down":
      return (sel + 1) % n;
    case "up":
      return (sel + n - 1) % n;
    case "pageDown":
      return Math.min(n - 1, sel + PAGE);
    case "pageUp":
      return Math.max(0, sel - PAGE);
  }
}

/** Alt+Down goes to the first row of the next group, Alt+Up to the first row of this group or, if already there, the one before; both wrap. */
export function moveSection<T extends Matchable>(shown: T[], query: string, sel: number, dir: 1 | -1): number {
  const heads = sectionHeads(shown, query);
  if (heads.length === 0) return sel;
  if (dir === 1) return heads.find((h) => h > sel) ?? heads[0]!;
  const before = heads.filter((h) => h < sel);
  return before.length ? before[before.length - 1]! : heads[heads.length - 1]!;
}
