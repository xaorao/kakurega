import { FORMATS, PRIVATE_KEY_BLOCKS } from "./formats.js";

const BODY = {
  alnum: (c) => /^[A-Za-z0-9]$/.test(c),
  "alnum-dash-underscore": (c) => /^[A-Za-z0-9_-]$/.test(c),
  "alnum-dash-underscore-dot": (c) => /^[A-Za-z0-9_.-]$/.test(c),
  hex: (c) => /^[0-9a-fA-F]$/.test(c),
  "upper-base32": (c) => /^[A-Z2-7]$/.test(c),
  base58: (c) => /^[A-Za-z0-9]$/.test(c) && !"0OIl".includes(c),
  "slack-like": (c) => /^[A-Za-z0-9-]$/.test(c),
};

const wordBoundary = (text, at) => at === 0 || !/[A-Za-z0-9_]/.test(text[at - 1]);

/** Names of the kinds of secret found in the text (never the secret itself). The same provider table the service uses
 * is loaded from formats.js, so a selection that looks like a key is refused here before it reaches even localhost. */
export function findSecrets(text) {
  const found = new Set();
  const t = String(text ?? "");
  for (const f of FORMATS) {
    let from = 0;
    for (;;) {
      const i = t.indexOf(f.prefix, from);
      if (i < 0) break;
      from = i + f.prefix.length;
      if (!wordBoundary(t, i)) continue;
      let n = 0;
      let body = "";
      while (from + n < t.length && BODY[f.body](t[from + n])) {
        body += t[from + n];
        n++;
      }
      const example = f.rule === "aws access key id" && body.includes("EXAMPLE");
      if (n >= f.min && n <= Math.max(f.max, f.min) && !example) {
        if (f.prefix === "SK" && !(/[0-9]/.test(body) && /[A-Za-z]/.test(body))) continue;
        found.add(f.rule);
      }
    }
  }
  for (const m of PRIVATE_KEY_BLOCKS) if (t.includes(m)) found.add("private key block");
  if (/\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/.test(t)) found.add("jwt");
  return [...found];
}
