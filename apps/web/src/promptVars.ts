/** Variables in a saved prompt, after LibreChat's (MIT) `{{name}}` and `{{name:one|two}}`: a field to fill for each before the
 *  prompt goes into the message box. Differences from it: the options start after the first colon only, so an option may
 *  hold a colon; a prompt is not filled while any field is empty, so no `{{name}}` is ever sent to a model; and the names
 *  that fill themselves are the date and time only. */

export interface PromptVar {
  /** As written between the braces, the key that fills it. */
  raw: string;
  name: string;
  options: string[];
}

const VAR = /{{([^{}\n]{1,80})}}/g;
/** Filled from the clock, never asked for. */
const AUTO = ["current_date", "current_datetime", "iso_datetime"];

/** The fields a prompt asks for, in the order they first appear, each once. */
export function variablesOf(text: string): PromptVar[] {
  const out: PromptVar[] = [];
  for (const m of text.matchAll(VAR)) {
    const raw = m[1] ?? "";
    const at = raw.indexOf(":");
    const name = (at >= 0 ? raw.slice(0, at) : raw).trim();
    if (!name || AUTO.includes(name.toLowerCase()) || out.some((v) => v.raw === raw)) continue;
    const options = at >= 0 ? raw.slice(at + 1).split("|").map((o) => o.trim()).filter(Boolean) : [];
    out.push({ raw, name, options });
  }
  return out;
}

const two = (n: number) => String(n).padStart(2, "0");

/** The prompt with every field filled, or the names still empty. The date and time come from `now`. */
export function fillPrompt(text: string, values: Record<string, string>, now: Date): { text: string } | { missing: string[] } {
  const missing = variablesOf(text).filter((v) => !(values[v.raw] ?? "").trim()).map((v) => v.name);
  if (missing.length > 0) return { missing };
  const auto: Record<string, string> = {
    current_date: `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())}`,
    current_datetime: `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())} ${two(now.getHours())}:${two(now.getMinutes())}`,
    iso_datetime: now.toISOString(),
  };
  // one pass over the text, so a value that itself looks like {{x}} is not filled again
  const filled = text.replace(VAR, (whole: string, raw: string) => {
    const key = raw.trim().toLowerCase();
    if (AUTO.includes(key)) return auto[key] ?? whole;
    return values[raw] !== undefined ? values[raw].trim() : whole;
  });
  return { text: filled };
}
