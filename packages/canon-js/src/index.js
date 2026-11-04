const SAFE_MAX = 9007199254740991n;

export class CanonError extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

function fail(code, message) {
  throw new CanonError(code, message);
}

export function parseStrict(input) {
  let text;
  if (typeof input === "string") {
    text = input;
  } else {
    try {
      text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(input);
    } catch {
      fail("invalid_utf8", "input is not valid UTF-8");
    }
  }
  let i = 0;
  const n = text.length;

  const ws = () => {
    while (i < n) {
      const c = text.charCodeAt(i);
      if (c === 0x20 || c === 0x09 || c === 0x0a || c === 0x0d) i++;
      else break;
    }
  };

  let depth = 0;
  const parseValue = () => {
    if (depth > 128) fail("too_deep", "nesting is too deep");
    ws();
    if (i >= n) fail("syntax", "unexpected end of input");
    const c = text[i];
    if (c === "{" || c === "[") {
      depth++;
      const r = c === "{" ? parseObject() : parseArray();
      depth--;
      return r;
    }
    if (c === '"') return parseString();
    if (c === "-" || (c >= "0" && c <= "9")) return parseNumber();
    if (text.startsWith("true", i)) { i += 4; return true; }
    if (text.startsWith("false", i)) { i += 5; return false; }
    if (text.startsWith("null", i)) { i += 4; return null; }
    fail("syntax", "unexpected character at " + i);
  };

  const parseObject = () => {
    i++;
    const map = new Map();
    ws();
    if (text[i] === "}") { i++; return map; }
    for (;;) {
      ws();
      if (text[i] !== '"') fail("syntax", "object key must be a string");
      const key = parseString();
      if (map.has(key)) fail("duplicate_key", "duplicate object key");
      ws();
      if (text[i] !== ":") fail("syntax", "expected ':'");
      i++;
      map.set(key, parseValue());
      ws();
      if (text[i] === ",") { i++; continue; }
      if (text[i] === "}") { i++; return map; }
      fail("syntax", "expected ',' or '}'");
    }
  };

  const parseArray = () => {
    i++;
    const arr = [];
    ws();
    if (text[i] === "]") { i++; return arr; }
    for (;;) {
      arr.push(parseValue());
      ws();
      if (text[i] === ",") { i++; continue; }
      if (text[i] === "]") { i++; return arr; }
      fail("syntax", "expected ',' or ']'");
    }
  };

  const hex4 = () => {
    const h = text.slice(i, i + 4);
    if (!/^[0-9a-fA-F]{4}$/.test(h)) fail("syntax", "bad \\u escape");
    i += 4;
    return parseInt(h, 16);
  };

  const parseString = () => {
    i++;
    let out = "";
    for (;;) {
      if (i >= n) fail("syntax", "unterminated string");
      const c = text[i];
      const code = text.charCodeAt(i);
      if (c === '"') { i++; return out; }
      if (code < 0x20) fail("syntax", "raw control character in string");
      if (c === "\\") {
        i++;
        const e = text[i++];
        if (e === '"' || e === "\\" || e === "/") out += e;
        else if (e === "b") out += "\b";
        else if (e === "f") out += "\f";
        else if (e === "n") out += "\n";
        else if (e === "r") out += "\r";
        else if (e === "t") out += "\t";
        else if (e === "u") {
          const u = hex4();
          if (u >= 0xd800 && u <= 0xdbff) {
            if (text[i] !== "\\" || text[i + 1] !== "u") fail("lone_surrogate", "lone high surrogate");
            i += 2;
            const lo = hex4();
            if (lo < 0xdc00 || lo > 0xdfff) fail("lone_surrogate", "high surrogate not followed by low surrogate");
            out += String.fromCharCode(u, lo);
          } else if (u >= 0xdc00 && u <= 0xdfff) {
            fail("lone_surrogate", "lone low surrogate");
          } else {
            out += String.fromCharCode(u);
          }
        } else fail("syntax", "bad escape");
        continue;
      }
      if (code >= 0xd800 && code <= 0xdfff) {
        const next = text.charCodeAt(i + 1);
        if (code <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) {
          out += text[i] + text[i + 1];
          i += 2;
          continue;
        }
        fail("lone_surrogate", "lone surrogate in input");
      }
      out += c;
      i++;
    }
  };

  const parseNumber = () => {
    const m = /^-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?/.exec(text.slice(i));
    if (!m) fail("syntax", "bad number");
    const lit = m[0];
    i += lit.length;
    if (i < n && /[0-9A-Za-z.+\-]/.test(text[i])) fail("syntax", "bad number");
    if (m[2] !== undefined || m[3] !== undefined) fail("float_forbidden", "floating-point numbers are forbidden");
    if (lit === "-0") fail("negative_zero", "negative zero is forbidden");
    const big = BigInt(lit);
    if (big > SAFE_MAX || big < -SAFE_MAX) fail("unsafe_integer", "integer outside the safe range");
    return big;
  };

  if (text.charCodeAt(0) === 0xfeff) fail("syntax", "byte order mark is not accepted");
  const v = parseValue();
  ws();
  if (i < n) fail("trailing_data", "data after the JSON value");
  return v;
}

function utf16Compare(a, b) {
  const len = Math.min(a.length, b.length);
  for (let k = 0; k < len; k++) {
    const d = a.charCodeAt(k) - b.charCodeAt(k);
    if (d !== 0) return d;
  }
  return a.length - b.length;
}

function writeString(s) {
  let out = '"';
  for (let k = 0; k < s.length; k++) {
    const c = s.charCodeAt(k);
    const ch = s[k];
    if (ch === '"') out += '\\"';
    else if (ch === "\\") out += "\\\\";
    else if (c === 0x08) out += "\\b";
    else if (c === 0x0c) out += "\\f";
    else if (c === 0x0a) out += "\\n";
    else if (c === 0x0d) out += "\\r";
    else if (c === 0x09) out += "\\t";
    else if (c < 0x20) out += "\\u" + c.toString(16).padStart(4, "0");
    else out += ch;
  }
  return out + '"';
}

export function serialize(v) {
  if (v === null) return "null";
  if (v === true) return "true";
  if (v === false) return "false";
  if (typeof v === "bigint") return v.toString();
  if (typeof v === "string") return writeString(v);
  if (Array.isArray(v)) return "[" + v.map(serialize).join(",") + "]";
  if (v instanceof Map) {
    const keys = [...v.keys()].sort(utf16Compare);
    return "{" + keys.map((k) => writeString(k) + ":" + serialize(v.get(k))).join(",") + "}";
  }
  fail("unsupported", "unsupported value");
}

export function canonicalize(input) {
  return serialize(parseStrict(input));
}

export async function digestHex(input) {
  const bytes = new TextEncoder().encode(canonicalize(input));
  const hash = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(hash)].map((b) => b.toString(16).padStart(2, "0")).join("");
}
