import test from "node:test";
import assert from "node:assert/strict";
import { hi } from "../src/locales/hi.ts";
import { serverText } from "../src/serverText.ts";

const t = (k, p = {}) => hi[k].replace(/\{(\w+)\}/g, (_, x) => String(p[x]));

test("st01 the service's fixed release sentences are shown in Hindi, and anything unknown exactly as it came", () => {
  assert.equal(serverText("1 withheld file", false, t), "1 रोकी गई फ़ाइल");
  assert.equal(serverText("3 withheld files", false, t), "3 रोकी गई फ़ाइलें");
  assert.equal(serverText("That no other defects exist: the review lists what it found, not what is absent.", false, t), "कि कोई और दोष नहीं है: समीक्षा वही बताती है जो उसे मिला, वह नहीं जो अनुपस्थित है।");
  assert.equal(serverText("Together with earlier releases, 4 of this project's 12 files (45% by size) would have been shown to this recipient, above the 30% review threshold.", false, t), "पहले की रिलीज़ के साथ, इस प्रोजेक्ट की 12 फ़ाइलों में से 4 (आकार के हिसाब से 45%) इस प्राप्तकर्ता को दिखाई जा चुकी होंगी, जो 30% की समीक्षा सीमा से ऊपर है।");
  assert.equal(serverText("A sentence the page has never seen.", false, t), "A sentence the page has never seen.");
  assert.equal(serverText("1 withheld file", true, t), "1 withheld file", "in English the service's words are shown as they are");
});
