import test from "node:test";
import assert from "node:assert/strict";
import { MAX_BYTES, tooLarge, transcriptText, validExportName } from "../src/transcript.ts";
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
  for (const ok of ["conversation", "call-1", "notes.md", "x.txt", "A_b-c9"]) assert.equal(validExportName(ok), true, ok);
  for (const bad of ["", "a.b", ".hidden", "a/b", "a b", "../x", "x.pdf", "x.", "a".repeat(61), "é", ".md", "a\u0000"]) assert.equal(validExportName(bad), false, bad);
});

test("tr04 size is counted in bytes", () => {
  assert.equal(tooLarge("a".repeat(MAX_BYTES)), false);
  assert.equal(tooLarge("a".repeat(MAX_BYTES + 1)), true);
  assert.equal(tooLarge("é".repeat(MAX_BYTES / 2 + 1)), true, "two bytes each");
});

test("tr05 the texts exist in both languages", () => {
  for (const k of ["chat.export", "chat.export.name", "chat.export.go", "chat.export.done", "chat.export.bad", "chat.export.large"]) assert.ok(en[k] && hi[k], k);
});
