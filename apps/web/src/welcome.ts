// The first visit: three short steps, each skippable, then the first review. What is remembered is only that this was shown.

export const STEPS = ["language", "theme", "setup"] as const;
export type Step = (typeof STEPS)[number];

const KEY = "kk-welcome";

/** Whether the welcome has been dealt with (finished or skipped) in this browser. Storage that cannot be read counts as not done, so it shows. */
export function welcomeDone(storage: Pick<Storage, "getItem"> | undefined): boolean {
  try {
    return storage?.getItem(KEY) === "done";
  } catch {
    return false;
  }
}

export function markWelcomeDone(storage: Pick<Storage, "setItem"> | undefined): void {
  try {
    storage?.setItem(KEY, "done");
  } catch {
    /* it will be shown again next visit, which is harmless */
  }
}

/** The step after this one, or null after the last. */
export function nextStep(s: Step): Step | null {
  const i = STEPS.indexOf(s);
  return i >= 0 && i + 1 < STEPS.length ? STEPS[i + 1]! : null;
}

export interface CheckLike {
  required?: boolean;
  ok: boolean;
}

/** What to say about the setup: how many required checks do not pass. Null when it could not be read. */
export function setupSummary(checks: readonly CheckLike[] | null | undefined): { problems: number } | null {
  if (!checks) return null;
  return { problems: checks.filter((c) => c.required !== false && !c.ok).length };
}
