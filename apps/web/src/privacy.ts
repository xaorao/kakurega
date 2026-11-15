import type { Key } from "./i18n";

// "Where your data goes" is read by people, not programs: a name a program gave itself, a kind of route or a count is put
// in words here before it is shown, and no snake_case name, code or bare number of thousands reaches the page.

/** A name a program gave itself ("search_issues", "list-reviews", "files.read") in words: its separators become spaces. */
export function plainName(raw: string): string {
  return raw.replace(/[_.]+|(?<=[\p{L}\p{N}])-(?=[\p{L}\p{N}])/gu, " ").replace(/\s+/g, " ").trim() || raw;
}

/** An outside program's tool as the log names it ("github/search_issues"): the tool and the server it belongs to, in words. */
export function plainProgram(to: string): { tool: string; server: string | null } {
  const at = to.indexOf("/");
  if (at <= 0 || at === to.length - 1) return { tool: plainName(to.replace(/\//g, " ")), server: null };
  return { tool: plainName(to.slice(at + 1)), server: plainName(to.slice(0, at)) };
}

/** Where a review's model was, from the kind of route the log names. A kind this page does not know is said as "elsewhere",
 *  never as its code and never as this computer. */
export function routeWords(kind: string): Key {
  if (kind === "verified_tee") return "out.to.tee";
  if (kind === "public") return "out.to.public";
  if (kind === "device_only") return "out.to.here";
  return "out.to.elsewhere";
}

/** A count with the separators of the reader's language ("29,117"; in Hindi the Indian grouping, "12,34,567"). */
export function countText(n: number, locale: string): string {
  return (Number.isFinite(n) ? n : 0).toLocaleString(locale === "hi" ? "hi-IN" : "en-US");
}

/**
 * Which sentence describes where the model runs. The address the service reports decides it, not the label a route gives
 * itself: a route that calls itself "device only" but points at another computer is described as running elsewhere.
 * Test mode comes first (nothing is sent to any model), and with no address nothing is claimed at all.
 */
export function modelKey(m: { class: string; simulated: boolean; host: string | null; stays_here: boolean }): Key {
  if (m.simulated) return "privacy.model.sim";
  if (m.host === null) return "privacy.model.unknown";
  if (m.stays_here) return "privacy.model.local";
  return m.class === "verified_tee" ? "privacy.model.tee" : "privacy.model.remote";
}
