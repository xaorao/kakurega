import test from "node:test";
import assert from "node:assert/strict";
import { MAX_CHARS, languageFor, tokenize } from "../src/highlight.ts";

const joined = (ts) => ts.map((t) => t.text).join("");
const classes = (lang, code) => tokenize(lang, code).filter((t) => t.cls).map((t) => `${t.cls}:${t.text}`);

test("hl01 fence labels name a language or nothing", () => {
  assert.equal(languageFor("python"), "python");
  assert.equal(languageFor("PY"), "python");
  assert.equal(languageFor("ts"), "javascript");
  assert.equal(languageFor("bash"), "shell");
  assert.equal(languageFor("json"), "json");
  assert.equal(languageFor("rs"), "rust");
  assert.equal(languageFor("sql"), "sql");
  assert.equal(languageFor("go"), "c");
  for (const none of ["", "  ", undefined, null, "klingon", "constructor", "__proto__", "toString", "hasOwnProperty"]) assert.equal(languageFor(none), null, String(none));
});

test("hl02 python: keywords, strings, numbers, comments, calls and triple quotes", () => {
  const got = classes("python", 'def f(x):  # note\n    return g(x, "a#b") + 3.5  # end\n"""doc\nstring"""\n');
  assert.deepEqual(got, ["kw:def", "fn:f", "com:# note", "kw:return", "fn:g", 'str:"a#b"', "num:3.5", "com:# end", 'str:"""doc\nstring"""']);
  assert.deepEqual(classes("python", "x = None if y else True"), ["kw:None", "kw:if", "kw:else", "kw:True"]);
  assert.deepEqual(classes("python", "classy = defn"), [], "a word that only starts like a keyword is not one");
});

test("hl03 javascript: block and line comments, template strings over lines, escapes", () => {
  const got = classes("javascript", "const a = `x\ny`; /* c\nd */ // e\nfoo('it\\'s', 1e3)");
  assert.deepEqual(got, ["kw:const", "str:`x\ny`", "com:/* c\nd */", "com:// e", "fn:foo", "str:'it\\'s'", "num:1e3"]);
});

test("hl04 json: a string before a colon is a key, other strings are values", () => {
  assert.deepEqual(classes("json", '{"a": "b", "c" : [1, true, null]}'), ['key:"a"', 'str:"b"', 'key:"c"', "num:1", "kw:true", "kw:null"]);
});

test("hl05 shell and sql and rust and c", () => {
  assert.deepEqual(classes("shell", 'if [ -n "$HOME" ]; then echo ${X} # hi\nfi'), ["kw:if", 'str:"$HOME"', "kw:then", "kw:echo", "fn:${X}", "com:# hi", "kw:fi"]);
  assert.deepEqual(classes("sql", "SELECT a FROM t WHERE b = 'x' -- why\n"), ["kw:SELECT", "kw:FROM", "kw:WHERE", "str:'x'", "com:-- why"]);
  assert.deepEqual(classes("rust", 'fn main() { let s = "hi"; }'), ["kw:fn", "fn:main", "kw:let", 'str:"hi"']);
  assert.deepEqual(classes("c", "int main(void) { return 0; } // x"), ["kw:int", "fn:main", "kw:void", "kw:return", "num:0", "com:// x"]);
});

test("hl05b a string that is not closed ends with its line, except where strings may run on", () => {
  assert.deepEqual(classes("python", "x = 'abc\ny = 2"), ["str:'abc", "num:2"]);
  assert.deepEqual(classes("javascript", "x = `abc\ny = 2"), ["str:`abc\ny = 2"]);
});

test("hl06 the pieces always join back into exactly the text, whatever it holds", () => {
  const alphabet = ["'", '"', "`", "#", "/", "*", "\\", "\n", "$", "{", "}", "(", ")", "1", ".", "a", "def ", "if ", "--", "\"\"\"", "/*", "*/", ":", " ", "\t", "é", "🙂", "\u0000"];
  let seed = 12345;
  const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
  for (const lang of [null, "python", "javascript", "rust", "json", "shell", "sql", "c"]) {
    for (let n = 0; n < 1500; n++) {
      const len = Math.floor(rnd() * 40);
      let s = "";
      for (let i = 0; i < len; i++) s += alphabet[Math.floor(rnd() * alphabet.length)];
      assert.equal(joined(tokenize(lang, s)), s, `${lang} ${JSON.stringify(s)}`);
    }
  }
  for (const s of ["", "\\", '"', "'''", "/*", '"""', "`", "$", "${", "1.", ".", "#"]) for (const lang of ["python", "javascript", "shell", "c", "sql", "json", "rust"]) assert.equal(joined(tokenize(lang, s)), s);
});

test("hl07 pieces are never empty and no piece is split without a reason", () => {
  for (const t of tokenize("python", "a = 1\nb = 'x'\n")) assert.ok(t.text.length > 0);
  assert.deepEqual(tokenize("python", "pass pass").map((t) => t.text), ["pass", " ", "pass"]);
});

test("hl08 an unknown language, or a block that is too large, is left plain and untouched", () => {
  assert.deepEqual(tokenize(null, "def x(): pass"), [{ text: "def x(): pass", cls: null }]);
  assert.deepEqual(tokenize("nope", "x"), [{ text: "x", cls: null }]);
  const big = "x = 1\n".repeat(Math.ceil(MAX_CHARS / 6) + 5);
  assert.equal(tokenize("python", big).length, 1);
  assert.equal(joined(tokenize("python", big)), big);
});

test("hl09 five thousand lines and hostile long lines are coloured quickly, in one pass", () => {
  const file = Array.from({ length: 5000 }, (_, i) => `    value_${i} = compute(${i}, "name ${i}")  # line ${i}`).join("\n").slice(0, MAX_CHARS);
  const t0 = performance.now();
  const ts = tokenize("python", file);
  const ms = performance.now() - t0;
  assert.ok(ts.length > 1000);
  assert.ok(ms < 250, `${ms} ms`);
  for (const nasty of ["'" + "\\".repeat(30000), '"'.repeat(30000), "/*".repeat(15000), "a(".repeat(20000), "(".repeat(30000) + " ".repeat(20000)]) {
    const t1 = performance.now();
    assert.equal(joined(tokenize("javascript", nasty)), nasty);
    assert.ok(performance.now() - t1 < 500, `slow on ${nasty.slice(0, 5)}`);
  }
});
