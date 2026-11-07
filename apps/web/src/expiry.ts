// How long a release can still be approved. The service enforces the deadline (an expired release cannot be approved or
// delivered); this is only what the page says about it, and it never says a release is valid unless it can show how long it has.

export type Left =
  | { state: "unknown" }
  | { state: "expired" }
  | { state: "soon"; secs: number }
  | { state: "valid"; secs: number };

/** Ten minutes or less is "soon": worth saying in a warning colour and words. */
export const SOON_SECS = 600;

/** `expires` and `now` are Unix seconds. Anything that is not a finite number is unknown, and unknown is never shown as valid. */
export function timeLeft(expires: number | undefined | null, now: number): Left {
  if (typeof expires !== "number" || !Number.isFinite(expires) || !Number.isFinite(now)) return { state: "unknown" };
  const secs = Math.floor(expires - now);
  if (secs <= 0) return { state: "expired" };
  return secs <= SOON_SECS ? { state: "soon", secs } : { state: "valid", secs };
}

/** "2 h 5 min", "4 min 12 s", "40 s": the two largest units that are not zero, never more. */
export function formatSpan(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  if (s < 60) return `${s} s`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return m > 0 ? `${h} h ${m} min` : `${h} h`;
  const r = s % 60;
  return r > 0 ? `${m} min ${r} s` : `${m} min`;
}

/** The difference between the service's clock and this page's, in seconds, to add to the page's clock. Zero when the service did not say. */
export function clockOffset(serverTime: number | undefined | null, pageNow: number): number {
  return typeof serverTime === "number" && Number.isFinite(serverTime) && Number.isFinite(pageNow) ? Math.round(serverTime - pageNow) : 0;
}
