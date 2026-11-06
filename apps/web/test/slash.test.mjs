import test from "node:test";
import assert from "node:assert/strict";
import { COMMANDS, afterInput, closedAtStart, matches, onKey, parseLine, slashQuery, validJob } from "../src/slash.ts";
import { en } from "../src/locales/en.ts";
import { hi } from "../src/locales/hi.ts";

test("sl01 the popover is for a box that is only a slash and word characters", () => {
  assert.equal(slashQuery("/"), "");
  assert.equal(slashQuery("/rev"), "rev");
  assert.equal(slashQuery("/open job-3"), null, "a space ends it");
  assert.equal(slashQuery("hello /rev"), null);
  assert.equal(slashQuery(""), null);
  assert.equal(slashQuery("/a\nb"), null);
  assert.equal(slashQuery("//x"), "/x");
});

test("sl02 every command has a unique name and a description in both languages", () => {
  const names = COMMANDS.map((c) => c.name);
  assert.equal(new Set(names).size, names.length);
  for (const c of COMMANDS) {
    assert.match(c.name, /^[a-z]+$/);
    assert.ok(en[c.about] && hi[c.about], c.about);
  }
  assert.ok(en["slash.none"] && hi["slash.none"]);
});

test("sl03 matching puts names that start with the text first, then looser matches, and nothing for a wrong word", () => {
  assert.equal(matches("").length, COMMANDS.length);
  assert.deepEqual(matches("rev").map((c) => c.name).slice(0, 2), ["review", "reviews"]);
  assert.equal(matches("APPR")[0].name, "approve", "case does not matter");
  assert.equal(matches("memory")[0].name, "memory");
  assert.ok(matches("rmbr").some((c) => c.name === "remember"), "letters in order");
  assert.deepEqual(matches("zzzz"), []);
  assert.deepEqual(matches("e/e"), []);
});

test("sl04 a line is a command only when its first word is exactly one of ours and an argument is allowed", () => {
  assert.deepEqual(parseLine("/approve")?.arg, "");
  assert.equal(parseLine("/approve")?.cmd.name, "approve");
  assert.deepEqual(parseLine("/open job-3"), { cmd: COMMANDS.find((c) => c.name === "open"), arg: "job-3" });
  assert.equal(parseLine("  /approve"), null, "leading space");
  assert.equal(parseLine("/Approve"), null, "case matters");
  assert.equal(parseLine("/approves"), null);
  assert.equal(parseLine("/memory extra words"), null, "this command takes none");
  assert.equal(parseLine("/etc/passwd is a file"), null, "an ordinary message that starts with a slash");
  assert.equal(parseLine("/open job-3\nrm -rf"), null, "one line only");
  assert.equal(parseLine("hello"), null);
  assert.equal(parseLine("/"), null);
});

test("sl05 only a review id as the service writes it can come from a command", () => {
  for (const ok of ["job-1", "job-12345"]) assert.equal(validJob(ok), true, ok);
  for (const bad of ["", "job-", "job-x", "../x", "job-1/../../etc", "job-1 ", "JOB-1", "job-1234567890", "#/x", "job-1?x=1", "javascript:alert(1)"]) assert.equal(validJob(bad), false, bad);
});

test("sl06 keys move around the ends, choose with Tab or Enter, and Escape closes until the text changes", () => {
  const s0 = closedAtStart;
  let step = onKey(s0, 3, { key: "ArrowDown" });
  assert.equal(step.kind, "move");
  assert.equal(step.state.active, 1);
  assert.equal(step.state.chosen, true);
  step = onKey({ active: 2, chosen: true, dismissed: false }, 3, { key: "ArrowDown" });
  assert.equal(step.state.active, 0, "wraps down");
  step = onKey({ active: 0, chosen: true, dismissed: false }, 3, { key: "ArrowUp" });
  assert.equal(step.state.active, 2, "wraps up");
  assert.equal(onKey({ active: 0, chosen: false, dismissed: false }, 3, { key: "n", ctrlKey: true }).state.active, 1);
  assert.equal(onKey({ active: 1, chosen: true, dismissed: false }, 3, { key: "p", ctrlKey: true }).state.active, 0);
  assert.equal(onKey({ active: -1, chosen: false, dismissed: false }, 3, { key: "ArrowUp" }).state.active, 2, "from no highlight, up goes to the last");
  assert.deepEqual(onKey({ active: 1, chosen: true, dismissed: false }, 3, { key: "Tab" }), { kind: "choose", index: 1 });
  assert.deepEqual(onKey({ active: 1, chosen: true, dismissed: false }, 3, { key: "Enter" }), { kind: "choose", index: 1 });
  assert.deepEqual(onKey({ active: 1, chosen: true, dismissed: false }, 3, { key: "Enter", isComposing: true }), { kind: "none" }, "an input method's Enter is not a choice");
  assert.equal(onKey(s0, 3, { key: "Escape" }).state.dismissed, true);
  assert.deepEqual(onKey({ active: -1, chosen: false, dismissed: false }, 0, { key: "Enter" }), { kind: "none" });
  assert.deepEqual(onKey(s0, 0, { key: "ArrowDown" }), { kind: "none" });
  assert.deepEqual(onKey(s0, 3, { key: "a" }), { kind: "none" });
  assert.deepEqual(onKey(s0, 3, { key: "n" }), { kind: "none" }, "n alone is a letter");
});

test("sl07 after typing the highlight follows the first row unless a key moved it, and Escape is forgotten", () => {
  assert.deepEqual(afterInput({ active: 2, chosen: false, dismissed: true }, 5), { active: 0, chosen: false, dismissed: false });
  assert.deepEqual(afterInput({ active: 2, chosen: true, dismissed: false }, 5), { active: 2, chosen: true, dismissed: false });
  assert.deepEqual(afterInput({ active: 4, chosen: true, dismissed: false }, 3), { active: 0, chosen: false, dismissed: false }, "the chosen row is gone");
  assert.deepEqual(afterInput({ active: 0, chosen: false, dismissed: false }, 0), { active: -1, chosen: false, dismissed: false });
});
