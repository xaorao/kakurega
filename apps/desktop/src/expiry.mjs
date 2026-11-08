// How long a release can still be approved, as the native approval window says it. The service enforces the deadline (an expired
// release cannot be approved or delivered); this is only what the owner is told, and it never says a release is valid unless it can
// show how long it has. The same rules as the web app's `expiry.ts`.

/** Ten minutes or less is "soon". */
export const SOON_SECS = 600;

/** `expires` and `now` are Unix seconds from the service. Anything that is not a finite number is unknown. */
export function timeLeft(expires, now) {
  if (typeof expires !== "number" || !Number.isFinite(expires) || typeof now !== "number" || !Number.isFinite(now)) return { state: "unknown" };
  const secs = Math.floor(expires - now);
  if (secs <= 0) return { state: "expired" };
  return secs <= SOON_SECS ? { state: "soon", secs } : { state: "valid", secs };
}

/** "2 h 5 min", "4 min 12 s", "40 s": the two largest units that are not zero, never more. */
export function formatSpan(secs) {
  const s = Math.max(0, Math.floor(secs));
  if (s < 60) return `${s} s`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return m > 0 ? `${h} h ${m} min` : `${h} h`;
  const r = s % 60;
  return r > 0 ? `${m} min ${r} s` : `${m} min`;
}

/** The line, or lines, the approval window shows about the deadline. */
export function deadlineLines(approval) {
  const left = timeLeft(approval?.expires, approval?.server_time);
  switch (left.state) {
    case "expired":
      return ["This release has expired and can no longer be approved."];
    case "soon":
      return [`This release can be approved for another ${formatSpan(left.secs)}.`, "! Only a few minutes are left."];
    case "valid":
      return [`This release can be approved for another ${formatSpan(left.secs)}.`];
    default:
      return ["The deadline could not be read here; the service refuses an approval that is too late."];
  }
}
