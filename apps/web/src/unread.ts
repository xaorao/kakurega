// Which kept conversations changed since this browser last looked at them. The service keeps when each was last changed; this remembers,
// per conversation, the change this browser had seen. Nothing here is sent anywhere.

export interface Listed {
  id: string;
  updated: number;
}

export type Seen = Record<string, number>;

const KEY = "kk-seen";
export const MAX_REMEMBERED = 500;

/** What was remembered, or null when nothing was (the first visit, storage cleared, or storage that cannot be read). */
export function loadSeen(storage: Pick<Storage, "getItem"> | undefined): Seen | null {
  try {
    const raw = storage?.getItem(KEY);
    if (!raw) return null;
    const v: unknown = JSON.parse(raw);
    if (v === null || typeof v !== "object" || Array.isArray(v)) return null;
    const out: Seen = {};
    for (const [k, n] of Object.entries(v as Record<string, unknown>)) if (typeof n === "number" && Number.isFinite(n) && k.length <= 80) out[k] = n;
    return out;
  } catch {
    return null;
  }
}

export function saveSeen(storage: Pick<Storage, "setItem"> | undefined, seen: Seen): void {
  try {
    const entries = Object.entries(seen).sort((a, b) => b[1] - a[1]).slice(0, MAX_REMEMBERED);
    storage?.setItem(KEY, JSON.stringify(Object.fromEntries(entries)));
  } catch {
    /* the markers come back as "seen" next time, which is harmless */
  }
}

/** Starts a memory from what is listed now: on the first visit nothing is "new", because the owner has not had a chance to look at anything. */
export function baseline(list: readonly Listed[]): Seen {
  return Object.fromEntries(list.map((l) => [l.id, l.updated]));
}

/** The conversations that changed (or appeared) since they were seen. The conversation open right now is never one of them. */
export function unreadIds(list: readonly Listed[], seen: Seen, open: string | null): Set<string> {
  const out = new Set<string>();
  for (const l of list) {
    if (l.id === open) continue;
    const s = seen[l.id];
    if (s === undefined || l.updated > s) out.add(l.id);
  }
  return out;
}

/** The memory after looking at one conversation (or after sending a message in it). */
export function markSeen(seen: Seen, id: string, updated: number): Seen {
  return { ...seen, [id]: Math.max(updated, seen[id] ?? 0) };
}
