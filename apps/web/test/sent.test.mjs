import assert from "node:assert/strict";
import test from "node:test";
import { sameWords } from "../src/sent.ts";

test("sn01 the words went as typed only when the last message is the user's and identical", () => {
  const m = (role, content) => ({ role, content });
  assert.equal(sameWords([m("system", "s"), m("user", "hello there")], "hello there"), true);
  assert.equal(sameWords([m("system", "s"), m("user", "hello there ")], "hello there"), false, "a trailing space is a difference");
  assert.equal(sameWords([m("system", "s"), m("user", "Hello there")], "hello there"), false, "case is a difference");
  assert.equal(sameWords([m("user", "hello"), m("assistant", "hello")], "hello"), false, "the last message must be the user's");
  assert.equal(sameWords([], "hello"), false);
  assert.equal(sameWords([m("user", "")], ""), true);
  assert.equal(sameWords([m("user", "a\nb")], "a\nb"), true, "line breaks count");
});
