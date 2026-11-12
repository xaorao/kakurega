/** A moment written so it cannot be misread: the month as a word ("12 Nov 2026, 14:05", or "Nov 12, 2026, 2:05 PM"
 *  where that is the reader's order), never "12/11/2026", which reads as two different days. In Hindi the Hindi forms. */
export function when(seconds: number, lang: string, opts: { dateOnly?: boolean } = {}): string {
  const reader = typeof navigator !== "undefined" ? navigator.language : "";
  const tag = lang === "hi" ? "hi-IN" : reader?.toLowerCase().startsWith("en") ? reader : "en";
  const style: Intl.DateTimeFormatOptions = opts.dateOnly ? { dateStyle: "medium" } : { dateStyle: "medium", timeStyle: "short" };
  try {
    return new Intl.DateTimeFormat(tag, style).format(new Date(seconds * 1000));
  } catch {
    return new Intl.DateTimeFormat("en", style).format(new Date(seconds * 1000));
  }
}

/** The time alone for a moment earlier today, else the date and time. */
export function whenToday(seconds: number, lang: string, now: number = Date.now()): string {
  const d = new Date(seconds * 1000);
  const today = new Date(now);
  if (d.getFullYear() === today.getFullYear() && d.getMonth() === today.getMonth() && d.getDate() === today.getDate()) {
    const tag = lang === "hi" ? "hi-IN" : "en";
    return new Intl.DateTimeFormat(tag, { timeStyle: "short" }).format(d);
  }
  return when(seconds, lang);
}
