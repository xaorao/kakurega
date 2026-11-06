import test from "node:test";
import assert from "node:assert/strict";
import { editPoint, againPoint } from "../src/branching.ts";

const t = [
  { role: "user", content: "first" },
  { role: "assistant", content: "answer one" },
  { role: "user", content: "second" },
  { role: "assistant", content: "answer two" },
];

test("br01 editing a message keeps everything before it and returns its text", () => {
  assert.deepEqual(editPoint(t, 0), { at: 0, text: "first" });
  assert.deepEqual(editPoint(t, 2), { at: 2, text: "second" });
});

test("br02 answering again keeps everything before the question and returns the question", () => {
  assert.deepEqual(againPoint(t, 1), { at: 0, text: "first" });
  assert.deepEqual(againPoint(t, 3), { at: 2, text: "second" });
});

test("br03 an action on the wrong kind of message, a missing one or a reply with no question is refused", () => {
  assert.equal(editPoint(t, 1), null, "a reply cannot be edited");
  assert.equal(againPoint(t, 0), null, "a question cannot be answered again");
  assert.equal(againPoint(t, 2), null);
  assert.equal(editPoint(t, 9), null);
  assert.equal(againPoint(t, -1), null);
  assert.equal(againPoint([{ role: "user", content: "a" }, { role: "user", content: "b" }], 1), null, "a question after a question is not a reply");
  assert.equal(againPoint([{ role: "assistant", content: "alone" }], 0), null, "an answer with nothing before it");
  assert.equal(againPoint([{ role: "assistant", content: "a" }, { role: "assistant", content: "b" }], 1), null);
});

test("br04 the text returned is exactly what was said, spaces and line breaks included", () => {
  const odd = [{ role: "user", content: "  two\n\nlines  " }];
  assert.equal(editPoint(odd, 0).text, "  two\n\nlines  ");
});
