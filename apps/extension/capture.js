import { call, cleanSelection, describeFailure, pageOf, selectionProblem } from "./lib.js";
import { findSecrets } from "./secrets.js";

export async function loadConfig(storage = chrome.storage.local) {
  const { port, token } = await storage.get(["port", "token"]);
  return { port, token };
}

export async function captureSelection(info, deps = {}) {
  const fetchImpl = deps.fetch ?? fetch;
  const config = deps.config ?? (await loadConfig());
  if (!config.token) return { ok: false, message: "Not paired. Open the Kakurega popup to pair." };
  const text = cleanSelection(info?.selectionText);
  // the reason is named: "too long" and "hidden characters" are fixed in different ways
  if (!text) return { ok: false, message: selectionProblem(info?.selectionText) ?? "That selection cannot be sent." };
  const secrets = findSecrets(text);
  if (secrets.length > 0) return { ok: false, message: `Not sent: the selection looks like a secret (${secrets.join(", ")}). Nothing left your browser.` };
  const page = pageOf(info?.pageUrl);
  if (!page) return { ok: false, message: "Only pages on http or https can be sent." };
  try {
    const r = await call(fetchImpl, config, "POST", "/ext/capture", { text, page });
    if (r.status === 202) return { ok: true, message: "Queued in Kakurega. Review it there before anything is saved." };
    return { ok: false, message: describeFailure(r.status, r.body) };
  } catch {
    return { ok: false, message: "Nothing answers at Kakurega's saved address on this computer. Start Kakurega; if it is running, open the extension, disconnect and pair again." };
  }
}
