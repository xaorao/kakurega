import { FORMATS, PRIVATE_KEY_BLOCKS } from "./formats.js";

const BODY = {
  alnum: (c) => /^[A-Za-z0-9]$/.test(c),
  "alnum-dash-underscore": (c) => /^[A-Za-z0-9_-]$/.test(c),
  "alnum-dash-underscore-dot": (c) => /^[A-Za-z0-9_.-]$/.test(c),
  hex: (c) => /^[0-9a-fA-F]$/.test(c),
  "upper-base32": (c) => /^[A-Z2-7]$/.test(c),
  base58: (c) => /^[A-Za-z0-9]$/.test(c) && !"0OIl".includes(c),
  "slack-like": (c) => /^[A-Za-z0-9-]$/.test(c),
  "base64-std": (c) => /^[A-Za-z0-9+/]$/.test(c),
  "base64-any": (c) => /^[A-Za-z0-9+/=_-]$/.test(c),
  "upper-alnum-dash": (c) => /^[A-Z0-9-]$/.test(c),
  "bech32-upper": (c) => "QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L".includes(c),
  "hex-underscore": (c) => /^[0-9a-fA-F_]$/.test(c),
  "url-path": (c) => /^[A-Za-z0-9@/-]$/.test(c),
  "upper-alnum": (c) => /^[A-Z0-9]$/.test(c),
  "lower-alnum": (c) => /^[a-z0-9]$/.test(c),
  "alnum-dot": (c) => /^[A-Za-z0-9.]$/.test(c),
  "word-eq-dot-dash": (c) => /^[A-Za-z0-9_=.-]$/.test(c),
};

/** A body that is a fill character or says what it is (`ghp_xxxx...`, `..._PUT_YOUR_TOKEN_HERE_...`) is documentation. */
function placeholderBody(body) {
  if ([...body].length >= 12 && new Set(body).size <= 3) return true;
  const l = body.toLowerCase();
  return body.length >= 16 && ["your", "example", "placeholder", "changeme", "dummy", "redacted", "token_here", "key_here"].some((w) => l.includes(w));
}

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
      if (n >= f.min && n <= Math.max(f.max, f.min) && !example && !placeholderBody(body)) {
        if (f.prefix === "SK" && !(/[0-9]/.test(body) && /[A-Za-z]/.test(body))) continue;
        found.add(f.rule);
      }
    }
  }
  for (const m of PRIVATE_KEY_BLOCKS) if (t.includes(m)) found.add("private key block");
  if (/\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/.test(t)) found.add("jwt");
  return [...found];
}
