// The break-it page. Kakurega's own secret detector (WebAssembly) runs in this tab on what the visitor writes, and the
// page shows what it would withhold and what a model would be sent. Whether the visitor's made-up secret is in what
// would be sent is judged the way the signed leak benchmark judges a leak (scripts/bench-leak.py, needles()). Nothing
// typed here is sent anywhere. The one request the page makes by itself asks GitHub's public API how many reports there
// are; it carries nothing typed here. A report leaves only when the visitor opens the link and submits it on GitHub.
import { withhold } from "./withhold.js";

const REPO = "xaorao/kakurega";
const MIN_SECRET = 4;
// GitHub answers a longer address with 414 URI Too Long
const MAX_URL = 7000;
const an = (name) => (/^[aeiou]/i.test(name) ? "an " : "a ") + name;
const el = (tag, text, cls) => {
  const n = document.createElement(tag);
  if (text !== undefined) n.textContent = text;
  if (cls) n.className = cls;
  return n;
};
const $ = (root, sel) => root.querySelector(sel);

// made-up material, put together here so no key-shaped text sits in this file
const IDEAS = [
  { label: "A key in a settings file", kind: "a key or token", secret: "sk_li" + "ve_51Mz8QpL2" + "kXv9Rt4Yw7Bn3Cd6", text: ["# billing/settings.py", 'STRIPE_KEY = "' + "sk_li" + "ve_51Mz8QpL2" + "kXv9Rt4Yw7Bn3Cd6" + '"', "DEBUG = False"].join("\n") },
  { label: "A password said as a password", kind: "a password", secret: "Tuesday-maple-42", text: "Notes from the stand-up.\nThe staging password is Tuesday-maple-42, do not share it.\nDeploy on Friday." },
  { label: "A password in plain words", kind: "a password", secret: "purple elephant", text: "Notes from the stand-up.\nBen said the staging password is purple elephant.\nDeploy on Friday." },
  { label: "A code with no name", kind: "a code or PIN", secret: "482913", text: "Notes from the stand-up.\nPunch in 482913 at the side entrance.\nDeploy on Friday." },
];

const utf8 = (s) => new TextEncoder().encode(s);
const base64 = (s) => btoa(String.fromCharCode(...utf8(s)));
const hex = (s) => [...utf8(s)].map((b) => b.toString(16).padStart(2, "0")).join("");
// Python's urllib.parse.quote(s, safe=""): every byte but letters, digits and _ . - ~ as %XX
const quote = (s) => [...utf8(s)].map((b) => (/[A-Za-z0-9_.~-]/.test(String.fromCharCode(b)) ? String.fromCharCode(b) : "%" + b.toString(16).toUpperCase().padStart(2, "0"))).join("");

/** Every form of the secret whose presence in what is sent counts as getting through, as the leak benchmark counts it:
 *  the whole value, each half of one of 16 characters or more, and each of those as written, in base64, hex and
 *  URL-encoded. Lengths are in characters, as Python counts them. */
export function forms(secret) {
  const chars = [...secret];
  const parts = [["the whole secret", secret]];
  if (chars.length >= 16) {
    const half = Math.floor(chars.length / 2);
    parts.push(["the first half of it", chars.slice(0, half).join("")], ["the second half of it", chars.slice(half).join("")]);
  }
  const out = [];
  for (const [part, value] of parts) {
    for (const [how, text] of [["as written", value], ["in base64", base64(value)], ["in hex", hex(value)], ["URL-encoded", quote(value)]]) {
      if (!out.some((f) => f.text === text)) out.push({ part, how, text });
    }
  }
  return out;
}

/** The ranges of a line that hold any of the forms, merged, so they can be marked. */
function ranges(line, all) {
  const found = [];
  for (const f of all) for (let at = line.indexOf(f.text); at >= 0; at = line.indexOf(f.text, at + 1)) found.push([at, at + f.text.length]);
  found.sort((a, b) => a[0] - b[0]);
  const merged = [];
  for (const r of found) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1]) last[1] = Math.max(last[1], r[1]);
    else merged.push([...r]);
  }
  return merged;
}

/** The address of a new GitHub issue with the report form filled in. It carries what would have been sent, with the
 *  lines the page withheld still withheld; when that is too long for an address, only the lines that hold the secret. */
export function reportUrl({ sent, secret, kind, lines, detector }) {
  const make = (text) => {
    const fields = { template: "break-it.yml", title: `Got through: ${kind}`, text, secret, kind, detector: String(detector) };
    return `https://github.com/${REPO}/issues/new?` + Object.entries(fields).map(([k, v]) => k + "=" + encodeURIComponent(v)).join("&");
  };
  let url = make(sent);
  if (url.length <= MAX_URL) return { url, shortened: false };
  const kept = sent.split("\n").filter((_, i) => lines.includes(i + 1));
  const note = "\n(shortened by the page: only the lines that hold the secret)";
  let text = kept.join("\n");
  url = make(text + note);
  while (url.length > MAX_URL && text.length > 0) {
    text = [...text].slice(0, Math.floor([...text].length * 0.8)).join("");
    url = make(text + "…" + note);
  }
  return { url, shortened: true };
}

async function loadDetector() {
  const { instance } = await WebAssembly.instantiate(await (await fetch("../kk_secrets.wasm")).arrayBuffer(), {});
  const x = instance.exports;
  const scan = (text) => {
    const bytes = utf8(text);
    const p = x.kk_alloc(bytes.length);
    new Uint8Array(x.memory.buffer, p, bytes.length).set(bytes);
    const answer = x.kk_scan(p, bytes.length);
    const len = new DataView(x.memory.buffer).getUint32(answer, true);
    const json = new TextDecoder().decode(new Uint8Array(x.memory.buffer, answer + 4, len));
    x.kk_free(p, bytes.length);
    x.kk_free(answer, 4 + len);
    return JSON.parse(json);
  };
  return { scan, version: x.kk_detector_version() };
}

function challenge(root) {
  const box = $(root, "[data-text]");
  const secretBox = $(root, "[data-secret]");
  const kindBox = $(root, "[data-kind]");
  const out = $(root, "[data-out]");
  const counts = $(root, "[data-counts]");
  const bytes = $(root, "[data-bytes]");
  const verdict = $(root, "[data-verdict]");
  const stamp = $(root, "[data-stamp]");
  const line = $(root, "[data-line]");
  const sub = $(root, "[data-sub]");
  const act = $(root, "[data-act]");
  const report = $(root, "[data-report]");
  const reportNote = $(root, "[data-report-note]");
  let detector;

  const say = (state, title, words) => {
    const changed = !verdict.classList.contains(state);
    verdict.className = "gate-verdict " + state;
    // the stamp comes down again when the verdict changes, not on every key
    if (changed) {
      stamp.style.animation = "none";
      void stamp.offsetWidth;
      stamp.style.animation = "";
    }
    stamp.textContent = state === "through" ? "✕" : "隠";
    line.textContent = title;
    sub.textContent = words;
    act.hidden = state !== "through";
  };

  const render = () => {
    const text = box.value;
    const secret = secretBox.value.trim();
    const prepared = withhold(text, detector.scan(text));
    const held = new Map(prepared.held.map((h) => [h.line, h.rule]));
    const all = secret.length >= MIN_SECRET ? forms(secret) : [];
    const hits = [];
    const rows = text.split("\n").map((l, i) => {
      const row = el("div", undefined, "gate-line");
      if (held.has(i + 1)) {
        row.classList.add("gone");
        const bar = el("span", undefined, "bar");
        bar.style.width = Math.max(4, Math.min(14, l.trim().length)) + "ch";
        const why = el("span", `withheld · holds ${an(held.get(i + 1))}`, "why");
        why.title = why.textContent;
        row.append(bar, why);
        return row;
      }
      const marks = ranges(l, all);
      for (const f of all) if (l.includes(f.text)) hits.push({ line: i + 1, ...f });
      let at = 0;
      for (const [s, e] of marks) {
        if (s > at) row.append(l.slice(at, s));
        row.append(el("mark", l.slice(s, e)));
        at = e;
      }
      if (at < l.length) row.append(l.slice(at));
      if (l === "") row.textContent = " ";
      return row;
    });
    out.replaceChildren(...rows);
    bytes.textContent = `${utf8(prepared.sent).length} bytes`;
    const n = prepared.held.length;
    const kept = text === "" ? 0 : text.split("\n").length - n;
    counts.replaceChildren(el("span", `${n} ${n === 1 ? "line" : "lines"} withheld`, "count stop"), el("span", `${kept} ${kept === 1 ? "line" : "lines"} would be sent`, "count go"));

    if (text.trim() === "") return say("wait", "Write some text first.", "Put a made-up secret in it, then name the secret below the text.");
    if (secret === "") return say("wait", "Name your secret.", "Write the made-up secret in the box below the text, and this says whether it would get through.");
    if ([...secret].length < MIN_SECRET) return say("wait", "Use a longer secret.", `A secret of fewer than ${MIN_SECRET} characters could match almost any text.`);
    if (!all.some((f) => text.includes(f.text))) return say("wait", "That secret is not in your text.", "Not as written, and not in base64, hex or URL-encoded. Check the spelling, or name the part of the text that is the secret.");
    if (hits.length === 0) return say("stopped", "Held.", "No form of your secret is in what would be sent: not as written, not either half, not in base64, hex or URL-encoded. Try another way of writing it.");
    const lines = [...new Set(hits.map((h) => h.line))];
    const first = hits[0];
    const how = first.part === "the whole secret" ? `your secret, ${first.how}` : `${first.part.replace(" of it", " of your secret")}, ${first.how}`;
    say("through", "Got through.", `A model would be sent ${how}, on ${lines.length === 1 ? "line" : "lines"} ${lines.join(", ")}.`);
    const { url, shortened } = reportUrl({ sent: prepared.sent, secret, kind: kindBox.value, lines, detector: detector.version });
    report.href = url;
    reportNote.textContent = shortened
      ? "It opens a GitHub issue holding only the lines with your secret: your text is too long for one link. Nothing is sent until you submit it there."
      : "It opens a GitHub issue holding what would have been sent, and only that: lines the page withheld stay withheld. Nothing is sent until you submit it there.";
  };

  return {
    async start() {
      try {
        detector = await loadDetector();
      } catch {
        out.replaceChildren(el("p", "The detector could not be loaded in this browser. The try-it page and the app run the same one.", "gate-fail"));
        for (const b of root.querySelectorAll("input, textarea, select, button")) b.disabled = true;
        say("wait", "No verdict in this browser.", "The page needs WebAssembly to run the detector, and it did not load.");
        return;
      }
      for (const n of document.querySelectorAll("[data-detector]")) n.textContent = String(detector.version);
      const pick = (idea) => {
        box.value = idea.text;
        secretBox.value = idea.secret;
        kindBox.value = idea.kind;
        render();
      };
      const ideas = document.querySelector("[data-ideas]");
      for (const idea of IDEAS) {
        const b = el("button", idea.label, "button");
        b.type = "button";
        b.addEventListener("click", () => pick(idea));
        ideas.append(b);
      }
      box.addEventListener("input", render);
      secretBox.addEventListener("input", render);
      kindBox.addEventListener("change", render);
      pick(IDEAS[0]);
    },
  };
}

/** The public count of reports: issues on the repository labelled break-it, and of those, the ones the maintainer has
 *  labelled got-through or held. Unauthenticated: GitHub allows a few searches a minute from one address. */
async function search(labels) {
  const q = ["repo:" + REPO, "is:issue", ...labels].join("+");
  const r = await fetch(`https://api.github.com/search/issues?q=${q}&per_page=100`, { credentials: "omit", referrerPolicy: "no-referrer" });
  if (!r.ok) throw new Error("no answer");
  const body = await r.json();
  if (!Number.isInteger(body?.total_count) || body.total_count < 0 || !Array.isArray(body.items) || body.incomplete_results) throw new Error("no count");
  return body;
}
const labelled = (issue, name) => (issue.labels ?? []).some((l) => (typeof l === "string" ? l : l?.name) === name);

export async function readCounts() {
  const all = await search(["label:break-it"]);
  if (all.items.length >= all.total_count) {
    const through = all.items.filter((i) => labelled(i, "got-through")).length;
    const held = all.items.filter((i) => labelled(i, "held") && !labelled(i, "got-through")).length;
    return { reports: all.total_count, through, held };
  }
  const through = await search(["label:break-it", "label:got-through"]);
  const held = await search(["label:break-it", "label:held", "-label:got-through"]);
  return { reports: all.total_count, through: through.total_count, held: held.total_count };
}

async function reports(root) {
  const line = $(root, "[data-reports-line]");
  const tally = $(root, "[data-tally]");
  line.textContent = "Reading the count of public reports from GitHub…";
  try {
    const c = await readCounts();
    const unchecked = Math.max(0, c.reports - c.through - c.held);
    for (const [k, v] of Object.entries({ reports: c.reports, through: c.through, held: c.held, unchecked })) $(tally, `[data-n="${k}"]`).textContent = String(v);
    $(tally, '[data-n="through"]').parentElement.classList.toggle("leak", c.through > 0);
    tally.hidden = false;
    line.textContent = c.reports === 0 ? "No reports yet. Read from GitHub just now." : `Read from GitHub just now: ${c.reports} ${c.reports === 1 ? "report" : "reports"}, ${unchecked} not checked yet.`;
  } catch {
    tally.hidden = true;
    line.textContent = "The live count could not be read from GitHub just now. The signed benchmark's numbers stand on their own, and every report is on GitHub.";
  }
}

// the functions above are also imported by the test with no page around them
if (typeof document !== "undefined") {
  const c = document.querySelector("[data-challenge]");
  if (c) void challenge(c).start();
  const r = document.querySelector("[data-reports]");
  if (r) void reports(r);
}
