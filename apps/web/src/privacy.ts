import type { Key } from "./i18n";

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
