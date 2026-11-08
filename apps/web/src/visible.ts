/**
 * Text from a model, a file or another program, made safe to put on the screen: characters that change the order text is drawn
 * in (direction overrides, embeddings and isolates, the direction marks), that show nothing (zero-width characters and the byte
 * order mark) or that end a line without a line break (the line and paragraph separators) become the replacement mark, so
 * nothing can make one word look like another. Ordinary text, line breaks and tabs are unchanged.
 */
const HIDDEN = /[\u200b-\u200f\u202a-\u202e\u2066-\u2069\u2028\u2029\ufeff]/g;

export function visible(text: string): string {
  return text.replace(HIDDEN, "\ufffd");
}
