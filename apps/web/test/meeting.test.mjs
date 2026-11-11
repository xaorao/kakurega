import test from "node:test";
import assert from "node:assert/strict";
import { fitBytes, plainTranscript } from "../src/meeting-text.ts";

test("mt20 a subtitle file is reduced to its spoken lines and a plain file is left exactly as it is", () => {
  const vtt = "WEBVTT - call\n\nNOTE made by a recorder\n\n1\n00:00:01.000 --> 00:00:04.000 align:start\nAsha: We ship on Friday.\n\n2\n00:00:04.000 --> 00:00:06.000\nBen: Agreed.\n12 people joined\n";
  assert.equal(plainTranscript("call.vtt", vtt), "Asha: We ship on Friday.\nBen: Agreed.\n12 people joined");
  const srt = "1\r\n00:00:01,000 --> 00:00:04,000\r\nAsha: 2026 is the year.\r\n\r\n2\r\n00:00:04,000 --> 00:00:06,000\r\n42\r\n";
  // a spoken line that is only a number looks like a counter and is dropped; that is the price of reading the format simply
  assert.equal(plainTranscript("call.SRT", srt), "Asha: 2026 is the year.");
  const plain = "1\n00:00 --> 00:01\nAsha: hello\n";
  assert.equal(plainTranscript("notes.txt", plain), plain);
});

test("mt21 a note that is too long is cut at a line end and never in the middle of a character", () => {
  assert.equal(fitBytes("short", 100), "short");
  const text = "# Title\n" + "- a point that was made\n".repeat(400);
  const cut = fitBytes(text, 4000);
  assert.ok(new TextEncoder().encode(cut).length <= 4000);
  assert.ok(cut.startsWith("# Title\n- a point") && cut.endsWith("- a point that was made"));
  const hindi = "बैठक".repeat(2000);
  const h = fitBytes(hindi, 4000);
  assert.ok(new TextEncoder().encode(h).length <= 4000 && h.length > 1000 && hindi.startsWith(h));
  assert.equal(new TextDecoder("utf-8", { fatal: true }).decode(new TextEncoder().encode(h)), h);
});
