// What to tell the owner after a note is forgotten: the copies this program knows about and cannot remove, by name only.

export interface ForgetResult {
  forgotten?: boolean;
  copies_known?: {
    reviews?: string[];
    exports?: string[];
    conversations?: { id?: unknown; why?: unknown }[];
    conversations_more?: unknown;
    prompt_history?: unknown;
    search_complete?: unknown;
  };
}

/** What a forgotten note left in the owner's conversations and typed history, as the card needs it. */
export interface LeftBehind {
  conversations: { id: string; why: "text" | "source" }[];
  more: number;
  history: number;
  incomplete: boolean;
}

/** Reads the conversations and history out of a forget answer; anything that is not a plain id, a known reason or a small count is dropped. */
export function leftBehind(r: ForgetResult | null | undefined): LeftBehind | null {
  const c = r?.copies_known;
  const conversations: LeftBehind["conversations"] = [];
  for (const x of c?.conversations ?? []) {
    if (typeof x?.id === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(x.id) && (x.why === "text" || x.why === "source")) {
      conversations.push({ id: x.id, why: x.why });
    }
  }
  const count = (v: unknown) => (typeof v === "number" && Number.isInteger(v) && v > 0 && v < 1_000_000 ? v : 0);
  const more = count(c?.conversations_more);
  const history = count(c?.prompt_history);
  const incomplete = c?.search_complete === false;
  if (conversations.length === 0 && more === 0 && history === 0 && !incomplete) return null;
  return { conversations: conversations.slice(0, 20), more, history, incomplete };
}

/** The names of known copies, reviews first, at most `max` shown and the rest counted. Anything that is not a plain short name is dropped. */
export function copyNames(r: ForgetResult | null | undefined, max = 6): { shown: string[]; more: number } {
  const ok = (x: unknown): x is string => typeof x === "string" && x.length > 0 && x.length <= 120 && !/[\u0000-\u001f\u007f<>]/.test(x);
  const all = [...(r?.copies_known?.reviews ?? []).filter(ok), ...(r?.copies_known?.exports ?? []).filter(ok)];
  return { shown: all.slice(0, max), more: Math.max(0, all.length - max) };
}
