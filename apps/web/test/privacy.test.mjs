import test from "node:test";
import assert from "node:assert/strict";
import { countText, modelKey, plainName, plainProgram, routeWords } from "../src/privacy.ts";

const m = (over) => ({ class: "device_only", simulated: false, host: "127.0.0.1", stays_here: true, ...over });

test("pv01 the sentence about the model follows where it runs, in a fixed order of caution", () => {
  assert.equal(modelKey(m({})), "privacy.model.local");
  assert.equal(modelKey(m({ simulated: true })), "privacy.model.sim", "test mode wins: nothing is sent");
  assert.equal(modelKey(m({ host: null, stays_here: false })), "privacy.model.unknown", "no address, no claim");
  assert.equal(modelKey(m({ class: "verified_tee", host: "cloud.example", stays_here: false })), "privacy.model.tee");
  assert.equal(modelKey(m({ class: "public", host: "api.example", stays_here: false })), "privacy.model.remote");
});

test("pv02 a route that calls itself device-only but is not at this computer is never described as staying here", () => {
  assert.equal(modelKey(m({ class: "device_only", host: "models.example.com", stays_here: false })), "privacy.model.remote");
  assert.notEqual(modelKey(m({ class: "device_only", stays_here: false })), "privacy.model.local");
});

test("pv03 only the address says it stays here, whatever the class", () => {
  assert.equal(modelKey(m({ class: "verified_tee", stays_here: true })), "privacy.model.local");
  assert.equal(modelKey(m({ class: "weird", stays_here: false })), "privacy.model.remote");
  assert.equal(modelKey(m({ class: "device_only", simulated: true, host: null, stays_here: false })), "privacy.model.sim");
});

test("pv04 a name a program gave itself is put in words: no snake_case, kebab-case or dotted name reaches the page", () => {
  assert.equal(plainName("search_issues"), "search issues");
  assert.equal(plainName("list-reviews"), "list reviews");
  assert.equal(plainName("files.read"), "files read");
  assert.equal(plainName("__start__review__"), "start review");
  assert.equal(plainName("GitHub"), "GitHub", "a proper name keeps its letters");
  assert.equal(plainName("a - b"), "a - b", "a dash between words is left alone");
  assert.equal(plainName("___"), "___", "a name with nothing but separators is shown rather than emptied");
  for (const raw of ["start_review", "request_approval", "meeting_notes", "try-to-break-it"]) assert.doesNotMatch(plainName(raw), /_|\w-\w/, raw);
});

test("pv05 an outside program's tool is named with its server, both in words", () => {
  assert.deepEqual(plainProgram("github/search_issues"), { tool: "search issues", server: "github" });
  assert.deepEqual(plainProgram("linear-mcp/list_issues"), { tool: "list issues", server: "linear mcp" });
  assert.deepEqual(plainProgram("search_issues"), { tool: "search issues", server: null });
  assert.deepEqual(plainProgram("/odd_one"), { tool: "odd one", server: null });
  assert.deepEqual(plainProgram("odd_one/"), { tool: "odd one", server: null });
});

test("pv06 the kind of route a review used is said in words, and a kind this page does not know is never called this computer", () => {
  assert.equal(routeWords("verified_tee"), "out.to.tee");
  assert.equal(routeWords("public"), "out.to.public");
  assert.equal(routeWords("device_only"), "out.to.here");
  for (const odd of ["?", "none", "local_unverified", "", "DEVICE_ONLY"]) assert.equal(routeWords(odd), "out.to.elsewhere", odd);
});

test("pv07 counts carry the separators of the reader's language", () => {
  assert.equal(countText(29117, "en"), "29,117");
  assert.equal(countText(1142, "en"), "1,142");
  assert.equal(countText(1234567, "hi"), "12,34,567");
  assert.equal(countText(7, "hi"), "7");
  assert.equal(countText(Number.NaN, "en"), "0");
});
