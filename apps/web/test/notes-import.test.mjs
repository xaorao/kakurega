import test from "node:test";
import assert from "node:assert/strict";
import { isNoteFile, notesFromFile, splitEntries, isLinkIndex } from "../src/notes-import.ts";

test("ni01 a Hermes memory file becomes one note per entry", () => {
  assert.deepEqual(splitEntries("first\n§\nsecond\nmore\n§\n\n§\n third \n"), ["first", "second\nmore", "third"]);
  assert.deepEqual(splitEntries("only one"), ["only one"]);
  assert.deepEqual(splitEntries("\n§\n"), []);
  const n = notesFromFile("USER.md", "Prefers short answers\n§\nWorks in IST");
  assert.deepEqual(n.map((x) => x.name), ["USER entry 1", "USER entry 2"]);
  assert.deepEqual(notesFromFile("memory.md", "Deploys use blue\n§\nTests need a secret").map((x) => x.text), ["Deploys use blue", "Tests need a secret"]);
});

test("ni02 a Claude Code index of links is not a note, but a plain MEMORY.md is", () => {
  assert.equal(isLinkIndex("# Memory\n- [user role](user_role.md) - who\n- [fb](fb.md)\n"), true);
  assert.equal(isLinkIndex("Use pnpm"), false);
  assert.equal(isLinkIndex(""), false);
  assert.deepEqual(notesFromFile("MEMORY.md", "- [a](a.md)\n"), []);
  assert.deepEqual(notesFromFile("MEMORY.md", "Use pnpm").map((x) => x.text), ["Use pnpm"]);
});

test("ni03 any other file is one note named after the file, and an empty one is nothing", () => {
  assert.deepEqual(notesFromFile("coffee.md", "  Standup at 9:30  "), [{ name: "coffee.md", text: "Standup at 9:30" }]);
  assert.deepEqual(notesFromFile("empty.md", "  \n"), []);
});

test("ni04 a Cursor rule file keeps its description as the first line and drops the frontmatter", () => {
  const rule = "---\ndescription: Keep comments minimal\nglobs: *,**/*\nalwaysApply: true\n---\n\n# Comments\n\n- Do not narrate the code.\n";
  const n = notesFromFile("no-excessive-comments.mdc", rule);
  assert.equal(n.length, 1);
  assert.ok(!n[0].text.includes("alwaysApply"), n[0].text);
  assert.ok(n[0].text.startsWith("Keep comments minimal"), n[0].text);
});

test("ni05 a Claude Code memory file uses its name and description and drops the frontmatter; a broken frontmatter keeps the text", () => {
  const n = notesFromFile("user_role.md", "---\nname: user_role\ndescription: who the user is\nmetadata:\n  type: user\n---\nThe user builds things.\n");
  assert.deepEqual(n, [{ name: "user_role", text: "who the user is\n\nThe user builds things." }]);
  const broken = notesFromFile("x.md", "---\nname: never closed\nBody here");
  assert.ok(broken[0].text.includes("Body here"), "an unclosed frontmatter keeps the text instead of dropping it");
  assert.deepEqual(notesFromFile("e.md", "---\nname: n\n---\n   \n"), [], "no body and no description is nothing");
  assert.deepEqual(notesFromFile("d.md", "---\ndescription: only this\n---\n"), [{ name: "d.md", text: "only this" }]);
});

test("ni06 the import card takes Markdown, Cursor rule files and text, and nothing else", () => {
  for (const ok of ["a.md", "A.MD", "rule.mdc", "notes.markdown", "plain.txt"]) assert.equal(isNoteFile(ok), true, ok);
  for (const no of ["image.png", "x.mdx", "archive.md.zip", "md", "noext"]) assert.equal(isNoteFile(no), false, no);
});
