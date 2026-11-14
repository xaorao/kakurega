// The replay on the landing page. Everything it shows is read from the capsule that a real review produced (the build
// step writes those facts into replay.json), and the last step verifies that capsule in this browser with the same code
// the check page uses. Nothing is sent anywhere: the only requests are for files of this site.
import { verifyCapsule } from "@kakurega/verifier";

const root = document.querySelector("[data-replay]");
if (root) start(root);

function el(tag, text, cls) {
  const n = document.createElement(tag);
  if (text !== undefined) n.textContent = text;
  if (cls) n.className = cls;
  return n;
}

async function start(root) {
  const tabs = [...root.querySelectorAll('[role="tab"]')];
  const panels = tabs.map((t) => document.getElementById(t.getAttribute("aria-controls")));
  const select = (i, focus) => {
    tabs.forEach((t, k) => {
      t.setAttribute("aria-selected", String(k === i));
      t.tabIndex = k === i ? 0 : -1;
      panels[k].hidden = k !== i;
    });
    if (focus) tabs[i].focus();
  };
  tabs.forEach((t, i) => {
    t.addEventListener("click", () => select(i, false));
    t.addEventListener("keydown", (e) => {
      const to = e.key === "ArrowRight" ? (i + 1) % tabs.length : e.key === "ArrowLeft" ? (i + tabs.length - 1) % tabs.length : e.key === "Home" ? 0 : e.key === "End" ? tabs.length - 1 : -1;
      if (to < 0) return;
      e.preventDefault();
      select(to, true);
    });
  });

  let facts;
  try {
    facts = await (await fetch("replay.json")).json();
  } catch {
    document.querySelector("[data-replay-note]").textContent = "The replay's facts could not be loaded. The capsule and the check page still work.";
    return;
  }
  root.querySelector('[data-fact="model"]').textContent = facts.model;
  root.querySelector('[data-fact="answers"]').textContent = `${facts.answers} answers`;
  const list = root.querySelector("[data-findings]");
  for (const f of facts.findings) {
    const li = el("li");
    const head = el("strong", f.title);
    head.append(el("span", "reproduced by the kernel", "pill live"));
    // shown as text only: nothing from a finding is ever treated as markup
    li.append(head, el("code", f.where), el("p", f.why));
    list.append(li);
  }
  const body = root.querySelector("[data-files]");
  for (const f of facts.files) {
    const tr = el("tr");
    tr.append(el("td", f.name), el("td", `${f.bytes} bytes`), el("td", f.sha256.slice(0, 16) + "…"));
    body.append(tr);
  }

  const verdict = root.querySelector("[data-verdict]");
  const checkButton = root.querySelector("[data-check]");
  const breakButton = root.querySelector("[data-break]");
  let bytes = null;
  const show = (r, changed) => {
    verdict.replaceChildren();
    if (r.valid) {
      verdict.append(el("div", "Valid: every signature checks out", "ok"));
      const ul = el("ul");
      ul.append(
        el("li", `Signed by ${r.signer}`),
        el("li", `${r.fileCount} files exactly as signed, ${r.claimCount} claims bound to them`),
        el("li", `${(r.receipts ?? []).length} answers signed by the model's own key`),
        el("li", "What this does not show: that a claim is true, or that no other defect exists."),
      );
      verdict.append(ul);
    } else {
      verdict.append(el("div", changed ? "Refused: the file is not what was signed" : "Not valid", "bad"));
      const why = { digest_mismatch: "a file in it is no longer what was signed", bad_signature: "a signature no longer matches its content", capsule_not_canonical: "its bytes are not in the one form a capsule may have", expired: "it has expired" }[r.reason] ?? "it is not a valid capsule";
      verdict.append(el("p", changed ? `One byte was changed, and the check failed: ${why}. A capsule cannot be edited after it is signed.` : `The check failed: ${why}.`));
    }
  };
  const run = async (changed) => {
    try {
      if (!globalThis.crypto?.subtle) throw new Error("no-crypto");
      if (!bytes) bytes = new Uint8Array(await (await fetch("sample/review.kkg")).arrayBuffer());
      let input = bytes;
      if (changed) {
        input = bytes.slice();
        input[Math.floor(input.length / 2)] ^= 0x01;
      }
      show(await verifyCapsule(input, { now: Math.floor(Date.now() / 1000) }), changed);
      breakButton.disabled = false;
    } catch (e) {
      verdict.replaceChildren(el("p", String(e?.message) === "no-crypto" ? "This browser does not give pages the functions the check needs. Open the page over https or in a current browser." : "The check could not run here. The check page and the command line do the same check."));
    }
  };
  checkButton.addEventListener("click", () => void run(false));
  breakButton.addEventListener("click", () => void run(true));
}
