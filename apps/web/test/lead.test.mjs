import test from "node:test";
import assert from "node:assert/strict";
import { splitLead } from "../src/lead.ts";
import { en } from "../src/locales/en.ts";
import { hi } from "../src/locales/hi.ts";

test("a screen's opening never folds away the sentence that says what leaves this computer", () => {
  for (const [name, dict] of [["en", en], ["hi", hi]]) {
    // the two screens whose last sentence says where files are sent
    for (const key of ["ex.lead", "code.lead"]) {
      const [shown, folded] = splitLead(dict[key], 1);
      const last = dict[key].trim().split(/(?<=[.!?।])\s+(?=\S)/).at(-1);
      assert.ok(shown.endsWith(last), `${name} ${key}: the last sentence is shown`);
      assert.ok(!folded.includes(last), `${name} ${key}: and is not behind the fold`);
      if (name === "en") assert.match(shown, /NEAR AI Cloud the files/);
    }
    // the screen whose middle says that what you write goes to the model: shown whole
    assert.deepEqual(splitLead(dict["break.lead"], 0, true), [dict["break.lead"].trim(), ""], `${name} break.lead is shown whole`);
    const [rt] = splitLead(dict["rt.lead"], 1);
    assert.ok(rt.endsWith(dict["rt.lead"].trim().split(/(?<=[.!?।])\s+(?=\S)/).at(-1)), `${name} rt.lead keeps its last sentence`);
  }
  // the plain case: first sentence shown, the rest folded; a short rest is not folded at all
  assert.deepEqual(splitLead("One. " + "Two is a much longer sentence that goes on for a good while to be folded away."), ["One.", "Two is a much longer sentence that goes on for a good while to be folded away."]);
  assert.deepEqual(splitLead("One. Two."), ["One. Two.", ""]);
});
