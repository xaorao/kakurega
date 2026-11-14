/** Up to two letters from a name's words, as a monogram: "Security reviewer" is SR, "Lawyer" is LA. */
export function initials(name: string): string {
  const words = name.split(/[\s-]+/).filter((w) => /\p{L}/u.test(w));
  const first = (w: string) => Array.from(w).find((c) => /\p{L}/u.test(c)) ?? "";
  const [a, b] = words;
  if (a !== undefined && b !== undefined) return (first(a) + first(b)).toUpperCase();
  const letters = Array.from(words[0] ?? "").filter((c) => /\p{L}/u.test(c));
  return letters.slice(0, 2).join("").toUpperCase();
}

/** A monogram for each name of one list, no two alike where the names allow it: "Data analyst" is DA, so
 *  "Devil's advocate" beside it is DE, and a third D-A name takes its first letter and its last word's second. */
export function marks(names: string[]): string[] {
  const letters = (w: string) => Array.from(w).filter((c) => /\p{L}/u.test(c));
  const taken = new Set<string>();
  return names.map((name) => {
    const words = name.split(/[\s-]+/).filter((w) => /\p{L}/u.test(w));
    const first = letters(words[0] ?? "");
    const last = letters(words[words.length - 1] ?? "");
    const tries = [initials(name), first.slice(0, 2).join("").toUpperCase(), ((first[0] ?? "") + (last[1] ?? "")).toUpperCase(), ((first[0] ?? "") + (last[last.length - 1] ?? "")).toUpperCase()];
    const mark = tries.find((m) => m.length > 0 && !taken.has(m)) ?? initials(name);
    taken.add(mark);
    return mark;
  });
}

/** One of six hues, the same for the same id every time. */
export function hueOf(id: string): number {
  let h = 0;
  for (const c of id) h = (h * 31 + c.codePointAt(0)!) >>> 0;
  return (h % 6) + 1;
}
