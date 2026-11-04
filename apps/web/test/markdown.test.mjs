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
