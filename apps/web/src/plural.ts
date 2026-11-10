// A text that counts something has a singular form when it is for exactly one: the key with ".one" added. A key without one is
// used for every count, which is how a text written with "file(s)" keeps working.

/** The key to use for a text whose count is `params.n` or `params.count`. `has` says which keys exist. */
export function pluralKey<K extends string>(key: K, params: Record<string, string | number>, has: (k: string) => boolean): K | string {
  const n = params.n ?? params.count;
  if (Number(n) === 1 && String(n).trim() === "1" && has(`${key}.one`)) return `${key}.one`;
  return key;
}
