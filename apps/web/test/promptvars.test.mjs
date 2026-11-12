import test from "node:test";
import assert from "node:assert/strict";
import { fillPrompt, variablesOf } from "../src/promptVars.ts";

test("pv01 each field once, in order, with its options; the date and time are not asked for", () => {
  const vs = variablesOf("Write to {{client}} in a {{tone:formal|casual|very: polite}} tone about {{client}}. Today is {{current_date}}.");
  assert.deepEqual(vs, [
    { raw: "client", name: "client", options: [] },
    { raw: "tone:formal|casual|very: polite", name: "tone", options: ["formal", "casual", "very: polite"] },
  ]);
  assert.deepEqual(variablesOf("No fields here, and {single} braces are text."), []);
});

test("pv02 a prompt is filled only when every field has a value, and a value is never filled again", () => {
  const text = "Write to {{client}} in a {{tone:formal|casual}} tone about {{client}} on {{current_date}}.";
  assert.deepEqual(fillPrompt(text, { client: "Acme" }, new Date(2026, 0, 5)), { missing: ["tone"] });
  assert.deepEqual(fillPrompt(text, { client: "  ", "tone:formal|casual": "x" }, new Date(2026, 0, 5)), { missing: ["client"] });
  const r = fillPrompt(text, { client: "{{tone:formal|casual}}", "tone:formal|casual": "formal" }, new Date(2026, 0, 5, 9, 7));
  assert.deepEqual(r, { text: "Write to {{tone:formal|casual}} in a formal tone about {{tone:formal|casual}} on 2026-01-05." });
  assert.deepEqual(fillPrompt("Plain prompt.", {}, new Date()), { text: "Plain prompt." });
});
