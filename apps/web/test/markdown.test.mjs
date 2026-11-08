import test from "node:test";
import assert from "node:assert/strict";
import { parse, inline, safeHref } from "../src/markdown.ts";

const flat = (b) => JSON.stringify(b);

test("md01 ordinary structure becomes blocks", () => {
  const b = parse("# Title\n\nSome **bold** and `code` text.\n\n- one\n- two\n\n1. first\n2. second\n\n```py\nprint(1)\n```\n> quoted");
  assert.deepEqual(b.map((x) => x.t), ["h", "p", "ul", "ol", "code", "quote"]);
  assert.equal(b[2].v.length, 2);
  assert.equal(b[4].lang, "py");
  assert.equal(b[4].v, "print(1)");
  assert.deepEqual(b[1].v.map((x) => x.t), ["text", "bold", "text", "code", "text"]);
});

test("md02 raw html and script text stay text and are never markup", () => {
  const b = parse('<script>alert(1)</script> <img src=x onerror=alert(1)> <a href="javascript:alert(1)">x</a>');
  assert.equal(b.length, 1);
  assert.deepEqual(b[0].v.map((x) => x.t), ["text"]);
  assert.ok(b[0].v[0].v.includes("<script>"), "the angle brackets are kept as text for the screen to escape");
});

test("md03 only plain https links are links; everything else stays visible text", () => {
  const ok = inline("see [docs](https://docs.rs/tokio/latest)");
  assert.equal(ok[1].t, "link");
  assert.equal(ok[1].host, "docs.rs");
  for (const bad of ["javascript:alert(1)", "data:text/html,x", "http://insecure.example", "https://u:p@evil.example/", "//evil.example", "file:///etc/passwd", "https://exa mple.com", "ftp://x.example", "https://x.example/\"onmouseover=1"]) {
    const r = inline(`[click](${bad})`);
    assert.ok(r.every((x) => x.t === "text"), `${bad} must not become a link: ${flat(r)}`);
    assert.ok(r.map((x) => x.v).join("").includes(bad), "the address stays visible so a reader can see where it pointed");
    assert.equal(safeHref(bad), null);
  }
});

test("md04 images are never requested", () => {
  const r = inline("![pixel](https://tracker.example/p.gif?u=secret) after");
  assert.ok(r.every((x) => x.t === "text"));
  assert.ok(!flat(r).includes("tracker.example"), "the address is not even shown, so nothing can be fetched or copied by accident");
  assert.ok(flat(r).includes("[image: pixel]"));
});

test("md05 unterminated or hostile input finishes and loses nothing important", () => {
  for (const h of ["```", "```js\nnever closed", "**", "`", "[a](", "[a](b", "![", "- ", "1.", "#", ">", "*".repeat(5000), "[".repeat(3000), "`".repeat(3000), "a\n".repeat(50000)]) {
    parse(h);
  }
  const big = parse("x".repeat(500000));
  assert.ok(big[0].v[0].v.length <= 200000, "input is capped");
  assert.equal(parse("```\nopen").at(0).v, "open", "an unclosed fence keeps its text");
});

test("md06 a model cannot hide text in a link label or fake a different host", () => {
  const r = inline("[https://bank.example](https://evil.example/login)");
  assert.equal(r[0].host, "evil.example", "the real host is carried so the screen shows it beside the label");
});

import { readFileSync } from "node:fs";
import { MAX_TABLE_COLUMNS, MAX_TABLE_ROWS, parse as parseMd, tableToTsv, plain } from "../src/markdown.ts";

const cell = (v) => [{ t: "text", v }];

test("md20 the web reader makes exactly the tree recorded for every shared case (the terminal test checks the other side)", () => {
  const cases = JSON.parse(readFileSync(new URL("../../../tests/fixtures/markdown_cases.json", import.meta.url), "utf8"));
  assert.ok(cases.length >= 40);
  // the recorded trees leave out where a link points to (`host`), as the terminal test does
  const noHost = (v) => JSON.parse(JSON.stringify(v), (k, x) => (k === "host" ? undefined : x));
  for (const c of cases) assert.deepEqual(noHost(parseMd(c.input)), noHost(c.blocks), JSON.stringify(c.input));
  assert.ok(cases.some((c) => c.terminal_blocks), "the one deliberate difference (tables) is recorded in the data");
});

test("md21 a table has a header, a delimiter row and rows, with alignment from the colons", () => {
  const [b] = parseMd("| Name | Qty | Note |\n|:--|--:|:-:|\n| apple | 3 | ok |\n| pear | 10 | late |");
  assert.equal(b.t, "table");
  assert.deepEqual(b.align, ["left", "right", "center"]);
  assert.deepEqual(b.head, [cell("Name"), cell("Qty"), cell("Note")]);
  assert.equal(b.rows.length, 2);
  assert.deepEqual(b.rows[1], [cell("pear"), cell("10"), cell("late")]);
  assert.deepEqual(parseMd("a | b\n--- | ---\nx | y")[0].align, [null, null], "outer pipes are optional");
});

test("md22 only a header with a matching delimiter row is a table; everything else stays the text it was", () => {
  for (const text of [
    "| a | b |\n| c | d |",
    "| a | b |\n|---|---|---|",
    "| a |\n|---|\n| 1 |",
    "a | b\nnot a delimiter",
    "just a line with a | pipe in it",
    "| a | b |\n|--x|---|",
    "| a | b |",
  ]) {
    assert.ok(!parseMd(text).some((b) => b.t === "table"), JSON.stringify(text));
  }
});

test("md23 rows are padded or cut to the header's width, an escaped pipe is a literal pipe, and a table ends at a blank line", () => {
  const [b, after] = parseMd("| a | b |\n|---|---|\n| 1 |\n| 1 | 2 | 3 |\n| x \\| y | z |\n\nafter");
  assert.deepEqual(b.rows[0], [cell("1"), []]);
  assert.deepEqual(b.rows[1], [cell("1"), cell("2")]);
  assert.deepEqual(b.rows[2], [cell("x | y"), cell("z")]);
  assert.deepEqual(after, { t: "p", v: cell("after") });
  const [t2, p2] = parseMd("| a | b |\n|---|---|\n| 1 | 2 |\nplain line without a pipe");
  assert.equal(t2.rows.length, 1);
  assert.equal(p2.t, "p");
});

test("md24 cells hold inline formatting but never markup, and a model cannot make an element or a request", () => {
  const [b] = parseMd("| a | b |\n|---|---|\n| **bold** `code` | <img src=x onerror=alert(1)> [x](https://example.com) ![i](https://evil.example/p.png) |");
  const [c0, c1] = b.rows[0];
  assert.deepEqual(c0.map((x) => x.t), ["bold", "text", "code"]);
  assert.ok(c1.some((x) => x.t === "text" && x.v.includes("<img src=x onerror=alert(1)>")), "markup stays text");
  assert.ok(c1.some((x) => x.t === "link" && x.href === "https://example.com/"));
  assert.ok(!JSON.stringify(c1).includes("evil.example/p.png") || JSON.stringify(c1).includes("[image:"), "an image is never loaded");
  assert.ok(JSON.stringify(parseMd("| a | b |\n|---|---|\n| [x](javascript:alert(1)) | y |")).includes("javascript:alert(1)"));
  assert.ok(!parseMd("| a | b |\n|---|---|\n| [x](javascript:alert(1)) | y |")[0].rows[0][0].some((x) => x.t === "link"));
});

test("md25 a huge table is cut to a hundred rows and twenty columns, and parses quickly", () => {
  const cols = 30;
  const head = "| " + Array.from({ length: cols }, (_, i) => `h${i}`).join(" | ") + " |";
  const delim = "|" + "---|".repeat(cols);
  const row = "| " + Array.from({ length: cols }, (_, i) => `c${i}`).join(" | ") + " |";
  const text = [head, delim, ...Array.from({ length: 400 }, () => row)].join("\n");
  const t0 = Date.now();
  const [b] = parseMd(text);
  assert.equal(b.head.length, MAX_TABLE_COLUMNS);
  assert.equal(b.align.length, MAX_TABLE_COLUMNS);
  assert.equal(b.rows.length, MAX_TABLE_ROWS);
  assert.ok(b.rows.every((r) => r.length === MAX_TABLE_COLUMNS));
  assert.ok(Date.now() - t0 < 2000);
});

test("md26 a table copies as tab-separated lines a spreadsheet can paste, with line breaks and tabs inside a cell made into spaces", () => {
  const [b] = parseMd("| Item | Cost |\n|---|--:|\n| **tea** | 3 |\n| a `b` c | [d](https://example.com) |");
  assert.equal(tableToTsv(b), "Item\tCost\ntea\t3\na b c\td (example.com)\n");
  const tabby = { t: "table", head: [cell("a\tb"), cell("c")], align: [null, null], rows: [[cell("x\ny"), cell("z")]] };
  assert.equal(tableToTsv(tabby), "a b\tc\nx y\tz\n");
  assert.equal(plain([{ t: "bold", v: [{ t: "em", v: cell("deep") }] }]), "deep");
});

test("md27 text that merely has pipes in a code block, a list or a quote is not turned into a table", () => {
  assert.ok(!parseMd("```\n| a | b |\n|---|---|\n| 1 | 2 |\n```").some((b) => b.t === "table"));
  assert.equal(parseMd("```\n| a | b |\n|---|---|\n```")[0].t, "code");
  assert.ok(!parseMd("- | a | b |\n- |---|---|").some((b) => b.t === "table"));
  assert.ok(!parseMd("> | a | b |\n> |---|---|").some((b) => b.t === "table"));
});
