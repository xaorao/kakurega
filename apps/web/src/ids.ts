// Long digests and ids are shown by their two ends, the way people compare them: the first 8 and the last 6 characters.
// The whole value is never lost: it is the element's title and what its copy button copies.

export const HEAD = 8;
export const TAIL = 6;

/** "3f9a2c1d…a07b42" for a value longer than its two ends and the mark between them; a short value as it is. A key written
 *  with its kind in front ("ed25519:3f9a…") keeps the kind whole and shortens what follows it, so the ends shown are the key's. */
export function shortId(value: string, head = HEAD, tail = TAIL): string {
  const kind = /^([a-z][a-z0-9]{1,11}:)(.+)$/.exec(value);
  if (kind) return kind[1]! + shortId(kind[2]!, head, tail);
  const chars = [...value];
  if (chars.length <= head + tail + 1) return value;
  return `${chars.slice(0, head).join("")}…${chars.slice(chars.length - tail).join("")}`;
}
