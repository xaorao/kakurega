/** Variables in a saved prompt, after LibreChat's (MIT) `{{name}}` and `{{name:one|two}}`: a field to fill for each before the
 *  prompt goes into the message box. Differences from it: the options start after the first colon only, so an option may
 *  hold a colon; a prompt is not filled while any field is empty, so no `{{name}}` is ever sent to a model; and the names
 *  that fill themselves are the date and time only. The terminal follows the same rules (`kk-tui/src/prompt_vars.rs`);
 *  both are checked against `test/promptvars-cases.json`. */

export interface PromptVar {
  /** As first written between the braces. */
  raw: string;
  /** The key that fills it: every place with this name takes the one value. */
  name: string;
  options: string[];
}

/** `{{` and `}}` around up to 80 characters with no brace and no line break; the innermost pair counts. */
const SPOT = /{{([^{}\n]+)}}/g;
const MAX = 80;
/** Filled from the clock, never asked for, whatever follows a colon. */
const AUTO = ["current_date", "current_datetime", "iso_datetime"];

const trim = (s: string) => s.replace(/^[\s﻿]+|[\s﻿]+$/g, "");
const nameOf = (raw: string) => trim(raw.includes(":") ? raw.slice(0, raw.indexOf(":")) : raw);
const isAuto = (name: string) => AUTO.includes(name.toLowerCase());
const fits = (raw: string) => [...raw].length <= MAX;
const has = (values: Record<string, string>, name: string) => Object.prototype.hasOwnProperty.call(values, name);

/** The fields a prompt asks for, in the order they first appear, one per name; the first options given for a name count. */
export function variablesOf(text: string): PromptVar[] {
  const out: PromptVar[] = [];
  for (const m of text.matchAll(SPOT)) {
    const raw = m[1] ?? "";
    if (!fits(raw)) continue;
    const name = nameOf(raw);
    if (!name || isAuto(name)) continue;
    const options = raw.includes(":") ? raw.slice(raw.indexOf(":") + 1).split("|").map(trim).filter(Boolean) : [];
    const seen = out.find((v) => v.name === name);
    if (seen) {
      if (seen.options.length === 0 && options.length > 0) seen.options = options;
      continue;
    }
    out.push({ raw, name, options });
  }
  return out;
}

const two = (n: number) => String(n).padStart(2, "0");

/** The prompt with every field filled (by name), or the names still empty. The date and time come from `now`. */
export function fillPrompt(text: string, values: Record<string, string>, now: Date): { text: string } | { missing: string[] } {
  const missing = variablesOf(text).filter((v) => !has(values, v.name) || !trim(values[v.name] ?? "")).map((v) => v.name);
  if (missing.length > 0) return { missing };
  const auto: Record<string, string> = {
    current_date: `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())}`,
    current_datetime: `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())} ${two(now.getHours())}:${two(now.getMinutes())}`,
    // to the second, as the terminal writes it
    iso_datetime: now.toISOString().replace(/\.\d{3}Z$/, "Z"),
  };
  // one pass over the text, so a value that itself looks like {{x}} is not filled again
  const filled = text.replace(SPOT, (whole: string, raw: string) => {
    if (!fits(raw)) return whole;
    const name = nameOf(raw);
    if (isAuto(name)) return auto[name.toLowerCase()] ?? whole;
    return name && has(values, name) ? trim(values[name] ?? "") : whole;
  });
  return { text: filled };
}
