// Colouring of code in answers and reproducers. A small scanner that reads the text once from start to end: no regular
// expressions with backtracking, no HTML is made, and the pieces it returns always join back into exactly the text it was given,
// so what is copied and what is read are the same bytes. Colour is only a hint: classes also change weight or slant, and a block
// that is too large, or a language it does not know, is left plain.
//
// The reference is OpenCode's markdown worker, which tokenises with `@pierre/highlights` (Apache-2.0) and sends [text, style] pairs.
// That library, and Shiki's oniguruma engine in general, may need WebAssembly, which this page's content security policy does
// not allow (NOT VERIFIED which applies); this scanner is written here instead and covers common shapes, not full grammars.

export type Cls = "kw" | "str" | "num" | "com" | "fn" | "key" | null;
export interface Token {
  text: string;
  cls: Cls;
}

interface Lang {
  keywords: ReadonlySet<string>;
  /** Words that are written in any case (SQL). */
  anyCase?: boolean;
  line: readonly string[];
  block?: readonly [string, string];
  /** Characters that open a string that ends at the same character or at the end of the line. */
  quotes: string;
  /** Characters that open a string that may run over several lines. */
  multiline?: string;
  /** A string opened with three of these runs until three of them. */
  triple?: string;
  /** `$name` and `${name}` are words (shell). */
  dollar?: boolean;
  /** A string followed by a colon is a key (JSON). */
  keys?: boolean;
}

const set = (s: string) => new Set(s.split(/\s+/).filter(Boolean));

const LANGS: Record<string, Lang> = {
  python: {
    keywords: set("False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield"),
    line: ["#"],
    quotes: "\"'",
    triple: "\"'",
  },
  javascript: {
    keywords: set("async await break case catch class const continue debugger default delete do else export extends false finally for from function if import in instanceof let new null of return static super switch this throw true try typeof undefined var void while with yield interface type enum implements public private protected readonly as"),
    line: ["//"],
    block: ["/*", "*/"],
    quotes: "\"'",
    multiline: "`",
  },
  rust: {
    keywords: set("as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while"),
    line: ["//"],
    block: ["/*", "*/"],
    quotes: "\"",
  },
  json: {
    keywords: set("true false null"),
    line: [],
    quotes: "\"",
    keys: true,
  },
  shell: {
    keywords: set("if then else elif fi for while until do done case esac in function return exit local export set unset cd echo"),
    line: ["#"],
    quotes: "\"'",
    dollar: true,
  },
  sql: {
    keywords: set("select from where and or not insert into values update set delete create table drop alter add join left right inner outer on group by order having limit offset as distinct union all null is in like between exists case when then else end primary key foreign references index"),
    anyCase: true,
    line: ["--"],
    block: ["/*", "*/"],
    quotes: "'\"",
  },
  c: {
    keywords: set("auto break case char const continue default do double else enum extern float for goto if inline int long register return short signed sizeof static struct switch typedef union unsigned void volatile while class public private protected namespace using new delete template typename bool true false nullptr"),
    line: ["//"],
    block: ["/*", "*/"],
    quotes: "\"'",
  },
};

const ALIASES: Record<string, string> = {
  py: "python", python3: "python",
  js: "javascript", jsx: "javascript", ts: "javascript", tsx: "javascript", typescript: "javascript", mjs: "javascript", node: "javascript",
  rs: "rust",
  sh: "shell", bash: "shell", zsh: "shell", console: "shell",
  jsonc: "json",
  c: "c", cpp: "c", "c++": "c", h: "c", java: "c", go: "c", cs: "c",
};

/** The language a fence or a file label names, or null when this scanner has no rules for it. */
export function languageFor(label: string | undefined | null): string | null {
  const k = (label ?? "").trim().toLowerCase();
  if (k === "") return null;
  const name = Object.prototype.hasOwnProperty.call(ALIASES, k) ? ALIASES[k]! : k;
  return Object.prototype.hasOwnProperty.call(LANGS, name) ? name : null;
}

/** Above this many characters the text is shown plain: the cost of colouring should never be able to hang the page. */
export const MAX_CHARS = 60_000;

const isIdStart = (c: string) => (c >= "a" && c <= "z") || (c >= "A" && c <= "Z") || c === "_" || c === "$";
const isDigit = (c: string) => c >= "0" && c <= "9";
const isIdPart = (c: string) => isIdStart(c) || isDigit(c);

/** The text cut into pieces, each with its kind. The pieces joined together are always exactly `code`. */
export function tokenize(lang: string | null, code: string): Token[] {
  const L = lang ? LANGS[lang] : undefined;
  if (!L || code.length > MAX_CHARS) return [{ text: code, cls: null }];
  const out: Token[] = [];
  let plain = "";
  const flush = () => {
    if (plain) out.push({ text: plain, cls: null });
    plain = "";
  };
  const push = (text: string, cls: Cls) => {
    flush();
    const last = out[out.length - 1];
    if (last && last.cls === cls && cls !== null && cls !== "fn" && cls !== "key") last.text += text;
    else out.push({ text, cls });
  };
  const n = code.length;
  let i = 0;
  while (i < n) {
    const c = code[i]!;
    // comments
    if (L.block && code.startsWith(L.block[0], i)) {
      const end = code.indexOf(L.block[1], i + L.block[0].length);
      const to = end < 0 ? n : end + L.block[1].length;
      push(code.slice(i, to), "com");
      i = to;
      continue;
    }
    const lc = L.line.find((m) => code.startsWith(m, i));
    if (lc !== undefined) {
      const nl = code.indexOf("\n", i);
      const to = nl < 0 ? n : nl;
      push(code.slice(i, to), "com");
      i = to;
      continue;
    }
    // strings
    if (L.triple && L.triple.includes(c)) {
      if (code.startsWith(c.repeat(3), i)) {
        const end = code.indexOf(c.repeat(3), i + 3);
        const to = end < 0 ? n : end + 3;
        push(code.slice(i, to), "str");
        i = to;
        continue;
      }
    }
    if (L.quotes.includes(c) || (L.multiline && L.multiline.includes(c))) {
      const multi = !!L.multiline && L.multiline.includes(c);
      let j = i + 1;
      while (j < n) {
        const d = code[j]!;
        if (d === "\\") {
          j += 2;
          continue;
        }
        if (d === c) {
          j += 1;
          break;
        }
        if (d === "\n" && !multi) break;
        j += 1;
      }
      const to = Math.min(j, n);
      const text = code.slice(i, to);
      if (L.keys) {
        let k = to;
        while (k < n && (code[k] === " " || code[k] === "\t")) k += 1;
        push(text, code[k] === ":" ? "key" : "str");
      } else {
        push(text, "str");
      }
      i = to;
      continue;
    }
    // numbers
    if (isDigit(c) || (c === "." && i + 1 < n && isDigit(code[i + 1]!))) {
      let j = i + 1;
      while (j < n && (isIdPart(code[j]!) || code[j] === ".")) j += 1;
      push(code.slice(i, j), "num");
      i = j;
      continue;
    }
    // shell variables
    if (L.dollar && c === "$") {
      let j = i + 1;
      if (code[j] === "{") {
        const end = code.indexOf("}", j);
        j = end < 0 ? n : end + 1;
      } else {
        while (j < n && isIdPart(code[j]!)) j += 1;
      }
      push(code.slice(i, j), "fn");
      i = j;
      continue;
    }
    // words
    if (isIdStart(c)) {
      let j = i + 1;
      while (j < n && isIdPart(code[j]!)) j += 1;
      const word = code.slice(i, j);
      const isKw = L.keywords.has(L.anyCase ? word.toLowerCase() : word);
      if (isKw) {
        push(word, "kw");
      } else {
        let k = j;
        while (k < n && (code[k] === " " || code[k] === "\t")) k += 1;
        if (code[k] === "(" && !L.dollar) push(word, "fn");
        else plain += word;
      }
      i = j;
      continue;
    }
    plain += c;
    i += 1;
  }
  flush();
  return out;
}
