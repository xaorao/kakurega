import test from "node:test";
import assert from "node:assert/strict";
import { notesFromFile, splitEntries, isLinkIndex } from "../src/notes-import.ts";

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
