// Composer rules. The draft cache and the history rule follow OpenCode's composer (MIT): packages/app/src/composer/persistence.tsx
// (one cached composer per conversation, at most 20), editor/interaction.ts (canNavigateHistory; Escape stops a running reply; focus returns to the box) and
// history/entry.ts (at most 100 entries, a repeat of the last one is not added).

const MAX_DRAFTS = 20;
const drafts = new Map<string, string>();
const MAX_HISTORY = 100;
/** Messages sent from this page, oldest first, each with the conversation it was sent in. */
let sent: { key: string; text: string }[] = [];

/** One draft per conversation: each kept conversation has its own, and an unkept chat has one. */
export function draftKey(session: string | null): string {
  return session ? `kept:${session}` : "new";
}

/** Keeps unsent text in memory while the page is open. Never in browser storage: a draft can hold anything the owner typed. */
export function saveDraft(key: string, text: string): void {
  drafts.delete(key);
  if (text.length === 0) return;
  drafts.set(key, text);
  while (drafts.size > MAX_DRAFTS) {
    const oldest = drafts.keys().next().value;
    if (oldest === undefined) break;
    drafts.delete(oldest);
  }
}

export function loadDraft(key: string): string {
  return drafts.get(key) ?? "";
}

export function forgetDraft(key: string): void {
  drafts.delete(key);
}

/** On sign-out nothing typed is left behind in the page: no drafts and no sent messages. */
export function clearDrafts(): void {
  drafts.clear();
  sent = [];
}

export function draftCount(): number {
  return drafts.size;
}

export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  isComposing: boolean;
}

/** Escape stops a reply that is being written. It never fires while an input method is composing (Hindi, Chinese, Japanese). */
export function stopsReply(e: KeyLike, busy: boolean): boolean {
  return busy && !e.isComposing && e.key === "Escape" && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey;
}

/** Remembers a sent message for Up and Down, only while the page is open. It is never stored anywhere: the chat promises
 *  that a conversation that is not kept is forgotten when the page closes or Clear is chosen. */
export function rememberSent(text: string, key = "new"): void {
  const t = text.trim();
  if (!t) return;
  const last = sent[sent.length - 1];
  if (last && last.text === t && last.key === key) return;
  sent.push({ key, text: t });
  if (sent.length > MAX_HISTORY) sent = sent.slice(-MAX_HISTORY);
}

export function sentHistory(): string[] {
  return sent.map((e) => e.text);
}

/** Clear and Forget remove a conversation, so Up must not bring its messages back. With no key, every message goes. */
export function forgetSent(key?: string): void {
  sent = key === undefined ? [] : sent.filter((e) => e.key !== key);
}

/** Keeping a conversation makes the messages already sent in it part of the kept one, so forgetting it removes them too. */
export function retagSent(from: string, to: string): void {
  sent = sent.map((e) => (e.key === from ? { key: to, text: e.text } : e));
}

/** Where Up and Down are in the sent messages: index -1 is not browsing, 0 is the newest. `saved` is what was in the box. */
export interface HistoryState {
  index: number;
  saved: string;
}

export function idleHistory(): HistoryState {
  return { index: -1, saved: "" };
}

/** Up starts from an empty box only; while browsing, Up and Down work from the start or the end of the recalled text.
 *  So the arrows never take caret movement away from a message being written. */
function canNavigate(direction: "up" | "down", text: string, cursor: number, inHistory: boolean): boolean {
  const position = Math.max(0, Math.min(cursor, text.length));
  if (inHistory) return position === 0 || position === text.length;
  if (direction === "up") return position === 0 && text.length === 0;
  return position === text.length;
}

/** The box after one Up or Down, or null when the key should move the caret as usual. */
export function historyStep(
  entries: readonly string[],
  state: HistoryState,
  direction: "up" | "down",
  text: string,
  cursor: number,
): { state: HistoryState; text: string; cursor: number } | null {
  const inHistory = state.index >= 0;
  if (!canNavigate(direction, text, cursor, inHistory)) return null;
  if (direction === "up") {
    if (state.index >= entries.length - 1) return null;
    const index = state.index + 1;
    const older = entries[entries.length - 1 - index];
    if (older === undefined) return null;
    return { state: { index, saved: inHistory ? state.saved : text }, text: older, cursor: 0 };
  }
  if (!inHistory) return null;
  if (state.index > 0) {
    const index = state.index - 1;
    const next = entries[entries.length - 1 - index];
    if (next === undefined) return null;
    return { state: { index, saved: state.saved }, text: next, cursor: next.length };
  }
  return { state: idleHistory(), text: state.saved, cursor: state.saved.length };
}

/** A plain Up or Down arrow, never during an input-method composition and never with a modifier. */
export function historyKey(e: KeyLike): "up" | "down" | null {
  if (e.isComposing || e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return null;
  return e.key === "ArrowUp" ? "up" : e.key === "ArrowDown" ? "down" : null;
}
