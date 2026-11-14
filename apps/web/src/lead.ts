/** A screen's opening text as sentences, in either language. */
function sentences(text: string): string[] {
  return text.trim().split(/(?<=[.!?।])\s+(?=\S)/);
}

/** Where a screen's opening text is cut: what is always shown, and what is one click away. The first sentence is always
 *  shown; so are the last `tail` sentences, which is where a screen says where its data goes. With `full`, or when the
 *  rest is short, nothing is folded. */
export function splitLead(text: string, tail = 0, full = false): [string, string] {
  const all = sentences(text);
  const first = all[0] ?? "";
  const kept = tail > 0 ? all.slice(Math.max(1, all.length - tail)) : [];
  const folded = all.slice(1, all.length - kept.length).join(" ");
  if (full || !first || folded.length < 60) return [text.trim(), ""];
  return [[first, ...kept].join(" "), folded];
}
