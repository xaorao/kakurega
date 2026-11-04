import { createSignal, type JSX } from "solid-js";
import { en } from "./locales/en";
import { hi } from "./locales/hi";

export type Key = keyof typeof en;
export type Locale = "en" | "hi";
export const LOCALES: [Locale, string][] = [["en", "English"], ["hi", "हिन्दी"]];
const catalogs: Record<Locale, Record<Key, string>> = { en, hi };

function initial(): Locale {
  try {
    const v = localStorage.getItem("kk-lang");
    if (v === "en" || v === "hi") return v;
  } catch {
    /* storage can be blocked */
  }
  return navigator.language?.toLowerCase().startsWith("hi") ? "hi" : "en";
}

const [locale, setLocaleSignal] = createSignal<Locale>(initial());
export { locale };

export function setLocale(l: Locale) {
  setLocaleSignal(l);
  document.documentElement.setAttribute("lang", l);
  try {
    localStorage.setItem("kk-lang", l);
  } catch {
    /* the choice still applies for this visit */
  }
}

export function applyLocale() {
  document.documentElement.setAttribute("lang", locale());
}

type Params = Record<string, string | number>;

export function t(key: Key, params: Params = {}): string {
  const text = catalogs[locale()][key] ?? catalogs.en[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (_, k) => String(params[k] ?? `{${k}}`));
}

const TAGS: Record<string, (children: JSX.Element, extra?: Record<string, JSX.Element>) => JSX.Element> = {
  b: (c) => <strong>{c}</strong>,
  c: (c) => <code>{c}</code>,
  e: (c) => <em>{c}</em>,
  a: (c) => (
    <a href="/verify" target="_blank" rel="noopener">
      {c}
    </a>
  ),
};

export function rich(key: Key, params: Params = {}): JSX.Element {
  const text = t(key, params);
  const out: JSX.Element[] = [];
  const re = /<([bcea])>(.*?)<\/\1>/g;
  let last = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    out.push(TAGS[m[1]!]!(m[2]));
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return <>{out}</>;
}
