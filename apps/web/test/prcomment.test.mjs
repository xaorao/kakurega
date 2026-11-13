import test from "node:test";
import assert from "node:assert/strict";
import { plain, prComment } from "../src/prcomment.ts";

const finding = (n, title, where, why) => ({ name: `finding-${n}.txt`, text: `Finding ${n}: ${title}\nLocation: ${where}\n\n${why}\n\nHow to check: run run-check.py (Python 3).\n\nWhat this establishes: the script printed CONTROL_OK.\n` });
const change = { name: "change.txt", text: `This review is of a change: commit bbbbbbbbbbbb measured from commit aaaaaaaaaaaa.\nreviewed commit: ${"b".repeat(40)}\nbase commit: ${"a".repeat(40)}\nfiles changed: 1 (0 added, 1 modified, 0 deleted)\n` };

test("pc01 the comment holds each finding, the commits and how to check, and nothing that was not released", () => {
  const files = [change, finding(1, "Token still valid after logout", "payments/auth/session.py:15", "logout() only pops a local dictionary."), { name: "repro-1.py", text: "SECRET_SCRIPT_BODY" }, { name: "sample-1.json", text: "SAMPLE_BODY" }];
  const c = prComment(files, "d".repeat(64));
  assert.ok(c.startsWith("### Private security review: 1 confirmed finding\n"));
  assert.ok(c.includes(`Reviewed commit \`${"b".repeat(40)}\`, measured from \`${"a".repeat(40)}\`.`));
  assert.ok(c.includes("**1. Token still valid after logout**"));
  assert.ok(c.includes("Where: `payments/auth/session.py:15`"));
  assert.ok(c.includes("logout() only pops a local dictionary."));
  assert.ok(c.includes("kaku verify review.kkg") && c.includes("capsule `" + "d".repeat(32) + "`") && !c.includes("d".repeat(33)));
  assert.ok(!c.includes("SECRET_SCRIPT_BODY") && !c.includes("SAMPLE_BODY"));
});

test("pc02 a whole-folder review names no commits, and several findings are counted", () => {
  const c = prComment([finding(1, "A", "a.py:1", "one"), finding(2, "B", "b.py:2", "two")], "e".repeat(64));
  assert.ok(c.startsWith("### Private security review: 2 confirmed findings\n"));
  assert.ok(!c.includes("Reviewed commit"));
  assert.ok(c.includes("**2. B**"));
});

test("pc03 text from a finding cannot notify people, link issues, or become markup", () => {
  const c = prComment([finding(1, "Ping @octocat and close #12 <img src=x>", "a`b.py:1", "# Heading\n- [click](http://evil.example) **bold** @team/all\n1. step\n> quote")], "f".repeat(64));
  assert.ok(!/(^|[^`])@octocat/.test(c) && c.includes("`@`octocat") && c.includes("`@`team/all"));
  assert.ok(c.includes("\\#12") && c.includes("\\<img src=x\\>"));
  assert.ok(c.includes("\\# Heading") && c.includes("\\- \\[click\\](http://evil.example) \\*\\*bold\\*\\*") && c.includes("\\1. step") && c.includes("\\> quote"));
  assert.ok(c.includes("Where: `a b.py:1`"));
  assert.equal(plain("a​b‮c"), "abc");
});

test("pc04 a release with no readable finding gives no comment", () => {
  assert.equal(prComment([change], "0".repeat(64)), null);
  assert.equal(prComment([{ name: "finding-1.txt", text: "something else" }], "0".repeat(64)), null);
  assert.equal(prComment([{ name: "finding-1.txt", text: null }], "0".repeat(64)), null);
});
