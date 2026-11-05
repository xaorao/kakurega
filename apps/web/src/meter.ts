// What the chat footer says about the size of the conversation. The figure is the model's own count when it gave one,
// and an estimate otherwise; the window is the context size asked of the model. Nothing is shown that the service did not send.

export interface Usage {
  context_tokens?: number;
  context_tokens_estimate: number;
  context_window?: number | null;
  estimate?: boolean;
}

export type MeterKey = "chat.context" | "chat.contextWindow" | "chat.contextCounted" | "chat.contextCountedWindow";

export interface MeterLine {
  key: MeterKey;
  n: number;
  max?: number;
  pct?: number;
  /** At or above 80% of the window: the owner should start a new conversation soon. */
  near: boolean;
}

export function meterLine(u: Usage): MeterLine {
  const counted = u.estimate === false && typeof u.context_tokens === "number" && u.context_tokens > 0;
  const n = counted ? (u.context_tokens as number) : (u.context_tokens ?? u.context_tokens_estimate);
  const max = typeof u.context_window === "number" && u.context_window > 0 ? u.context_window : undefined;
  if (max === undefined) return { key: counted ? "chat.contextCounted" : "chat.context", n, near: false };
  const pct = Math.min(999, Math.round((n * 100) / max));
  return { key: counted ? "chat.contextCountedWindow" : "chat.contextWindow", n, max, pct, near: pct >= 80 };
}

export type PageKey = "web.used" | "web.trimmed" | "web.leftOut";

/** What the footer says about the chosen web pages: how many were used, cut short or left out to fit the window. */
export function pageNotes(r: { web_sources_used: number; web_sources_trimmed?: number; web_sources_left_out?: number }): { key: PageKey; n: number }[] {
  const out: { key: PageKey; n: number }[] = [];
  if (r.web_sources_used > 0) out.push({ key: "web.used", n: r.web_sources_used });
  if ((r.web_sources_trimmed ?? 0) > 0) out.push({ key: "web.trimmed", n: r.web_sources_trimmed ?? 0 });
  if ((r.web_sources_left_out ?? 0) > 0) out.push({ key: "web.leftOut", n: r.web_sources_left_out ?? 0 });
  return out;
}

/** What the footer says about recalled notes that did not fit the room kept for notes. */
export function noteNotes(r: { notes_left_out?: number }): { key: "chat.notesLeftOut"; n: number }[] {
  const n = r.notes_left_out ?? 0;
  return n > 0 ? [{ key: "chat.notesLeftOut", n }] : [];
}
