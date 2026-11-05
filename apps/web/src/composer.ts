// Composer rules. The draft cache and the history rule follow OpenCode's composer (MIT): packages/app/src/composer/persistence.tsx
// (one cached composer per conversation, at most 20) and editor/interaction.ts (Escape stops a running reply; focus returns to the box).

const MAX_DRAFTS = 20;
const drafts = new Map<string, string>();

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

/** On sign-out nothing typed is left behind in the page. */
export function clearDrafts(): void {
  drafts.clear();
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
