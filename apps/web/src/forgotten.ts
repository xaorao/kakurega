// What to tell the owner after a note is forgotten: the copies this program knows about and cannot remove, by name only.

export interface ForgetResult {
  forgotten?: boolean;
  copies_known?: { reviews?: string[]; exports?: string[] };
}

/** The names of known copies, reviews first, at most `max` shown and the rest counted. Anything that is not a plain short name is dropped. */
export function copyNames(r: ForgetResult | null | undefined, max = 6): { shown: string[]; more: number } {
  const ok = (x: unknown): x is string => typeof x === "string" && x.length > 0 && x.length <= 120 && !/[\u0000-\u001f\u007f<>]/.test(x);
  const all = [...(r?.copies_known?.reviews ?? []).filter(ok), ...(r?.copies_known?.exports ?? []).filter(ok)];
  return { shown: all.slice(0, max), more: Math.max(0, all.length - max) };
}
