// What the chat footer says about the size of the conversation. The figure is the model's own count when it gave one,
// and an estimate otherwise; the window is the context size asked of the model. Nothing is shown that the service did not send.

export interface Usage {
  context_tokens?: number;
  context_tokens_estimate: number;
  context_window?: number | null;
  estimate?: boolean;
}

export interface AnswerUsage {
  elapsed_ms?: unknown;
  reply_tokens_estimate?: unknown;
}

export interface AnswerLine {
  key: "chat.answer" | "chat.answerSpeed";
  tokens: number;
  took: string;
  per?: number;
}

const whole = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= 0;

/** What the footer says about the last answer: about how long it is and how long it took. The speed is a whole-answer figure
 *  (the time includes reading notes and building the prompt), shown only when the answer took at least half a second and the
 *  speed is at least one token a second. A missing, negative, fractional or non-number figure, or an answer with no length,
 *  gives no line at all. The same rules as the terminal's footer. */
export function answerLine(u: AnswerUsage): AnswerLine | null {
  if (!whole(u.elapsed_ms) || !whole(u.reply_tokens_estimate) || u.reply_tokens_estimate === 0) return null;
  const ms = u.elapsed_ms;
  const tokens = u.reply_tokens_estimate;
  const took = ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
  const per = ms >= 500 ? Math.floor((tokens * 1000) / ms) : 0;
  return per >= 1 ? { key: "chat.answerSpeed", tokens, took, per } : { key: "chat.answer", tokens, took };
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

export type SavedKey = "chat.saved" | "chat.notSaved" | "chat.notSavedForgotten" | "chat.notSavedError";

/** What the footer says about saving an exchange in a kept conversation: the real reason, never "full" when the service
 *  said otherwise. An older service that sends no reason keeps the old wording. */
export function savedLine(r: { saved?: boolean; not_saved?: string | null }): SavedKey {
  if (r.saved) return "chat.saved";
  if (r.not_saved === "forgotten") return "chat.notSavedForgotten";
  if (r.not_saved === "error") return "chat.notSavedError";
  return "chat.notSaved";
}

export type SummaryKey = "chat.summarisedNow" | "chat.summarised" | "chat.summaryFailed";

/** What the footer says about the summary standing in for the oldest turns of a kept conversation. */
export function summaryNotes(u: { history_turns_summarized?: number; compacted_now?: boolean; compaction_failed?: string | null }): { key: SummaryKey; n: number }[] {
  const n = u.history_turns_summarized ?? 0;
  const out: { key: SummaryKey; n: number }[] = [];
  if (n > 0) out.push({ key: u.compacted_now ? "chat.summarisedNow" : "chat.summarised", n });
  if (u.compaction_failed) out.push({ key: "chat.summaryFailed", n: 0 });
  return out;
}
