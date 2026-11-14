
/** "an OpenAI API key", "a GitHub token": the right article for a rule's name. */
const an = (name) => (/^[aeiou]/i.test(name) ? "an " : "a ") + name;
// What leaves the page: the text with every line a hit touches replaced. Kept apart from the page so it can be tested
// with the real detector and no browser.

/** The text with every line that holds a hit replaced, and what was replaced. Offsets from the detector are in bytes. */
export function withhold(text, hits) {
  const bytes = new TextEncoder().encode(text);
  const lines = text.split("\n");
  const starts = [];
  let at = 0;
  for (const line of lines) {
    starts.push(at);
    at += new TextEncoder().encode(line).length + 1;
  }
  const lineOf = (offset) => {
    let i = starts.length - 1;
    while (i > 0 && starts[i] > offset) i--;
    return i;
  };
  const held = new Map();
  for (const h of hits) {
    if (h.start >= bytes.length) continue;
    for (let i = lineOf(h.start); i <= lineOf(Math.max(h.start, h.end - 1)); i++) if (!held.has(i)) held.set(i, h.rule);
  }
  const sent = lines.map((line, i) => (held.has(i) ? `[line ${i + 1} was withheld on the user's computer: it holds ${an(held.get(i))}]` : line));
  return { sent: sent.join("\n"), held: [...held.entries()].map(([i, rule]) => ({ line: i + 1, rule })) };
}
