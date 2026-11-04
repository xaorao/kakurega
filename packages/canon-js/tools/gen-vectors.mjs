import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import canonicalize from "canonicalize";

const accept = [
  ["sorts keys", '{"b":1,"a":2}'],
  ["nested sorting", '{"outer":{"z":[3,{"y":1,"x":2}],"a":null}}'],
  ["empty members participate", '{"e":{},"a":[],"n":null,"k":1}'],
  ["scalars at top level", '"bare"'],
  ["true", "true"],
  ["false", "false"],
  ["null", "null"],
  ["empty array", "[]"],
  ["whitespace ignored", ' \n\t{ "a" : [ 1 , 2 ] ,\r\n "b" : { } } '],
  ["zero", "0"],
  ["safe max", "9007199254740991"],
  ["safe min", "-9007199254740991"],
  ["negative int", '{"n":-17}'],
  ["string escapes", '"q\\" b\\\\ s\\/ b\\b f\\f n\\n r\\r t\\t"'],
  ["control chars use lowercase \\u", '"\\u0001\\u001f\\u001F"'],
  ["delete char is literal", '"\\u007f"'],
  ["non ascii literal", '"café 😀"'],
  ["surrogate pair escape equals literal", '"\\ud83d\\ude00"'],
  ["key order astral before private use", '{"\\ud83d\\ude00":1,"\\ue000":2}'],
  ["key order astral before U+FFFF", '{"\\uffff":1,"\\ud800\\udc00":2}'],
  ["key order bmp mix", '{"b":1,"B":2,"é":3,"a":4,"Z":5}'],
  ["array order kept", "[3,1,2,[],{}]"],
  ["escaped key equals literal key", '{"\\u0062":1,"a":2}'],
  ["deep nesting", '{"a":{"a":{"a":{"a":{"a":[[[[1]]]]}}}}}'],
  ["number like strings stay strings", '{"x":"1.0","y":"1e5"}'],
  ["unicode normalisation is not applied", '{"e\\u0301":1,"\\u00e9":2}'],
];

const reject = [
  ["float", "1.5", "float_forbidden"],
  ["integral float", "1.0", "float_forbidden"],
  ["exponent", "1e2", "float_forbidden"],
  ["capital exponent", "1E2", "float_forbidden"],
  ["negative zero", "-0", "negative_zero"],
  ["unsafe integer", "9007199254740992", "unsafe_integer"],
  ["unsafe negative", "-9007199254740992", "unsafe_integer"],
  ["huge integer", "12345678901234567890123", "unsafe_integer"],
  ["duplicate key", '{"a":1,"a":2}', "duplicate_key"],
  ["duplicate key via escape", '{"a":1,"\\u0061":2}', "duplicate_key"],
  ["trailing comma object", '{"a":1,}', "syntax"],
  ["trailing comma array", "[1,]", "syntax"],
  ["single quotes", "{'a':1}", "syntax"],
  ["NaN", "NaN", "syntax"],
  ["leading zero", "01", "syntax"],
  ["plus sign", "+1", "syntax"],
  ["hex number", "0x10", "syntax"],
  ["lone high surrogate escape", '"\\ud800"', "lone_surrogate"],
  ["lone low surrogate escape", '"\\udc00"', "lone_surrogate"],
  ["trailing data", "{}x", "trailing_data"],
  ["two values", "1 2", "trailing_data"],
  ["byte order mark", "﻿{}", "syntax"],
  ["empty input", "", "syntax"],
  ["comment", '{"a":1}//x', "trailing_data"],
  ["raw newline in string", '"a\nb"', "syntax"],
  ["bad escape", '"\\x41"', "syntax"],
  ["unterminated string", '"abc', "syntax"],
  ["unterminated object", '{"a":1', "syntax"],
  ["nesting beyond the limit", "[".repeat(130) + "]".repeat(130), "too_deep"],
];

const out = { accept: [], reject: [] };
for (const [name, input] of accept) {
  const canonical = canonicalize(JSON.parse(input));
  const sha256 = createHash("sha256").update(canonical, "utf8").digest("hex");
  out.accept.push({ name, input, canonical, sha256 });
}
for (const [name, input, code] of reject) out.reject.push({ name, input, code });
out.reject.push({ name: "invalid utf-8 bytes", input_hex: "7b2261223a22ff227d", code: "invalid_utf8" });
writeFileSync(new URL("../../../fixtures/canon/vectors.json", import.meta.url), JSON.stringify(out, null, 2) + "\n");
console.log("accept", out.accept.length, "reject", out.reject.length);
