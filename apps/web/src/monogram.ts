/** Up to two letters from a name's words, as a monogram: "Security reviewer" is SR, "Lawyer" is LA. */
export function initials(name: string): string {
  const words = name.split(/[\s-]+/).filter((w) => /\p{L}/u.test(w));
  const first = (w: string) => Array.from(w).find((c) => /\p{L}/u.test(c)) ?? "";
  const [a, b] = words;
  if (a !== undefined && b !== undefined) return (first(a) + first(b)).toUpperCase();
  const letters = Array.from(words[0] ?? "").filter((c) => /\p{L}/u.test(c));
  return letters.slice(0, 2).join("").toUpperCase();
}

/** One of six hues, the same for the same id every time. */
export function hueOf(id: string): number {
  let h = 0;
  for (const c of id) h = (h * 31 + c.codePointAt(0)!) >>> 0;
  return (h % 6) + 1;
}
