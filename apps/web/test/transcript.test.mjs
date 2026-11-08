import test from "node:test";
import assert from "node:assert/strict";
import { MAX_BYTES, exportFileName, tooLarge, transcriptIn, transcriptJson, transcriptMarkdown, transcriptText, validExportName } from "../src/transcript.ts";
import { en } from "../src/locales/en.ts";
import { hi } from "../src/locales/hi.ts";

test("tr01 a conversation is written as You and Assistant blocks in order", () => {
  assert.equal(transcriptText([{ role: "user", content: "hi" }, { role: "assistant", content: "hello\nthere  " }]), "You: hi\n\nAssistant: hello\nthere\n");
  assert.equal(transcriptText([]), "\n");
});

test("tr02 control characters are dropped but line breaks and tabs stay", () => {
  assert.equal(transcriptText([{ role: "user", content: "a\u001b[2Jb\u0007c\td\ne" }]), "You: a[2Jbc\td\ne\n");
});

test("tr03 only names the service accepts are offered", () => {
  for (const ok of ["conversation", "call-1", "notes.md", "x.txt", "x.json", "A_b-c9"]) assert.equal(validExportName(ok), true, ok);
  for (const bad of ["", "a.b", ".hidden", "a/b", "a b", "../x", "x.pdf", "x.", "a".repeat(61), "é", ".md", ".json", "x.jsonl", "x.JSON", "a\u0000"]) assert.equal(validExportName(bad), false, bad);
});

test("tr04 size is counted in bytes", () => {
  assert.equal(tooLarge("a".repeat(MAX_BYTES)), false);
  assert.equal(tooLarge("a".repeat(MAX_BYTES + 1)), true);
  assert.equal(tooLarge("é".repeat(MAX_BYTES / 2 + 1)), true, "two bytes each");
});

test("tr05 the texts exist in both languages", () => {
  for (const k of ["chat.export", "chat.export.name", "chat.export.go", "chat.export.done", "chat.export.bad", "chat.export.large"]) assert.ok(en[k] && hi[k], k);
});

test("tr06 markdown puts a heading above each speaker and keeps the words, dropping only control characters", () => {
  const turns = [{ role: "user", content: "hi\u001b[2J" }, { role: "assistant", content: "hello\nthere  " }];
  assert.equal(transcriptMarkdown(turns), "## You\n\nhi[2J\n\n## Assistant\n\nhello\nthere\n");
  assert.equal(transcriptMarkdown([]), "\n");
});

test("tr07 the JSON export parses back to the same turns and carries a version marker, nothing else", () => {
  const turns = [{ role: "user", content: "a \"quoted\" line\nnext\ttab" }, { role: "assistant", content: "ok" }];
  const parsed = JSON.parse(transcriptJson(turns));
  assert.deepEqual(parsed, { kakurega_transcript: 1, turns });
  assert.deepEqual(Object.keys(parsed.turns[0]), ["role", "content"], "no page notes or controls travel with it");
  const odd = JSON.parse(transcriptJson([{ role: "user", content: "x\u0007y\u001bz" }]));
  assert.equal(odd.turns[0].content, "xyz");
  assert.deepEqual(JSON.parse(transcriptJson([])), { kakurega_transcript: 1, turns: [] });
  assert.equal(transcriptIn("json", turns), transcriptJson(turns));
  assert.equal(transcriptIn("markdown", turns), transcriptMarkdown(turns));
  assert.equal(transcriptIn("lines", turns), transcriptText(turns));
});

test("tr08 a JSON export always asks for a .json name, the others keep the name as typed", () => {
  assert.equal(exportFileName("call", "json"), "call.json");
  assert.equal(exportFileName("call.md", "json"), "call.json");
  assert.equal(exportFileName("call.txt", "json"), "call.json");
  assert.equal(exportFileName("call.json", "json"), "call.json");
  assert.equal(exportFileName("a.md.md", "json"), "a.md.json");
  assert.equal(exportFileName("call", "lines"), "call");
  assert.equal(exportFileName("call.txt", "markdown"), "call.txt");
  assert.equal(validExportName(exportFileName("x".repeat(60), "json")), false, "a name that is too long once .json is added is refused before it is sent");
});

test("tr09 the format texts exist in both languages", () => {
  for (const k of ["chat.export.format", "chat.export.format.lines", "chat.export.format.markdown", "chat.export.format.json"]) assert.ok(en[k] && hi[k], k);
});
