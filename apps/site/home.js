// The landing page's two live parts. The gate runs Kakurega's own secret detector (WebAssembly) in this tab on whatever
// is typed, shows what would be sent with those lines withheld, and seals it: a SHA-256 of exactly what would leave.
// Nothing typed here is sent anywhere. The second part switches between pictures of the app.
import { withhold } from "./withhold.js";
/** "an OpenAI API key", "a GitHub token": the right article for a rule's name. */
const an = (name) => (/^[aeiou]/i.test(name) ? "an " : "a ") + name;


const SAMPLE = [
  "# payments-service · settings.py",
  'STRIPE_KEY = "' + "sk_li" + "ve_51Mz8QpL2" + "kXv9Rt4Yw7Bn3Cd6" + '"',
  'AWS_KEY = "' + "AKIA" + "Q7XK2M9P" + "LD4RT6WZ" + '"',
  "# the admin password is " + "maple" + "-otter-" + "4471",
  "",
  "def logout(token):",
  '    cookie = {"session": token}',
  '    cookie.pop("session", None)   # the server keeps the session',
].join("\n");

const el = (tag, text, cls) => {
  const n = document.createElement(tag);
  if (text !== undefined) n.textContent = text;
  if (cls) n.className = cls;
  return n;
};
const hex = (buf) => [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
const short = (h) => h.slice(0, 16) + "…" + h.slice(-16);

async function loadDetector() {
  const { instance } = await WebAssembly.instantiate(await (await fetch("kk_secrets.wasm")).arrayBuffer(), {});
  const x = instance.exports;
  return (text) => {
    const bytes = new TextEncoder().encode(text);
    const p = x.kk_alloc(bytes.length);
    new Uint8Array(x.memory.buffer, p, bytes.length).set(bytes);
    const answer = x.kk_scan(p, bytes.length);
    const len = new DataView(x.memory.buffer).getUint32(answer, true);
    const json = new TextDecoder().decode(new Uint8Array(x.memory.buffer, answer + 4, len));
    x.kk_free(p, bytes.length);
    x.kk_free(answer, 4 + len);
    return JSON.parse(json);
  };
}

async function gate(root) {
  const box = root.querySelector("[data-gate-in]");
  const out = root.querySelector("[data-gate-out]");
  const counts = root.querySelector("[data-gate-counts]");
  const bytes = root.querySelector("[data-gate-bytes]");
  const sealButton = root.querySelector("[data-gate-seal]");
  const verdict = root.querySelector("[data-gate-verdict]");
  box.value = SAMPLE;
  let scan;
  try {
    scan = await loadDetector();
  } catch {
    out.replaceChildren(el("p", "The detector could not be loaded in this browser. The try-it page and the app run the same one.", "gate-fail"));
    sealButton.disabled = true;
    return;
  }
  let sealed = null;
  let current = "";
  const digest = async (text) => hex(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
  const showVerdict = async () => {
    if (sealed === null) return;
    const now = await digest(current);
    const intact = now === sealed;
    verdict.hidden = false;
    verdict.classList.toggle("refused", !intact);
    root.querySelector("[data-gate-stamp]").textContent = intact ? "隠" : "✕";
    root.querySelector("[data-gate-line]").textContent = intact ? "Valid. Sealed on this page." : "Refused. The bytes changed.";
    root.querySelector("[data-gate-sub]").textContent = intact ? "This shows these exact bytes were sealed. It does not show that anything in them is right." : "What would leave no longer matches the seal. A receiver would reject it.";
    root.querySelector("[data-gate-sealed]").textContent = "sha256:" + short(sealed);
    root.querySelector("[data-gate-now]").textContent = "sha256:" + short(now);
    sealButton.textContent = intact ? "Sealed" : "Seal again";
  };
  const render = () => {
    const text = box.value;
    const prepared = withhold(text, scan(text));
    const held = new Map(prepared.held.map((h) => [h.line, h.rule]));
    current = prepared.sent;
    const rows = text.split("\n").map((line, i) => {
      const row = el("div", undefined, "gate-line");
      if (held.has(i + 1)) {
        row.classList.add("gone");
        const bar = el("span", undefined, "bar");
        bar.style.width = Math.max(6, Math.min(26, line.trim().length)) + "ch";
        const why = el("span", `withheld · holds ${an(held.get(i + 1))}`, "why");
        why.title = why.textContent;
        row.append(bar, why);
      } else row.textContent = line === "" ? " " : line;
      return row;
    });
    out.replaceChildren(...rows);
    bytes.textContent = `${new TextEncoder().encode(prepared.sent).length} bytes`;
    const n = prepared.held.length;
    const kept = text.split("\n").length - n;
    counts.replaceChildren(el("span", `${n} ${n === 1 ? "line" : "lines"} withheld`, "count stop"), el("span", `${kept} ${kept === 1 ? "line" : "lines"} would be sent`, "count go"));
    void showVerdict();
  };
  box.addEventListener("input", render);
  sealButton.addEventListener("click", async () => {
    sealed = await digest(current);
    void showVerdict();
  });
  render();
}

function tabs(root) {
  const all = [...root.querySelectorAll('[role="tab"]')];
  const panels = all.map((t) => document.getElementById(t.getAttribute("aria-controls")));
  const select = (i, focus) => {
    all.forEach((t, k) => {
      t.setAttribute("aria-selected", String(k === i));
      t.tabIndex = k === i ? 0 : -1;
      panels[k].hidden = k !== i;
    });
    if (focus) all[i].focus();
  };
  all.forEach((t, i) => {
    t.addEventListener("click", () => select(i, false));
    t.addEventListener("keydown", (e) => {
      const to = e.key === "ArrowRight" ? (i + 1) % all.length : e.key === "ArrowLeft" ? (i + all.length - 1) % all.length : e.key === "Home" ? 0 : e.key === "End" ? all.length - 1 : -1;
      if (to < 0) return;
      e.preventDefault();
      select(to, true);
    });
  });
}

const g = document.querySelector("[data-gate]");
if (g) void gate(g);
const s = document.querySelector("[data-shots]");
if (s) tabs(s);
