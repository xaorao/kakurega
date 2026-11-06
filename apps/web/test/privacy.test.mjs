import test from "node:test";
import assert from "node:assert/strict";
import { modelKey } from "../src/privacy.ts";

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
