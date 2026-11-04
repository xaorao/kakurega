import { call, cleanSelection, describeFailure, pageOf } from "./lib.js";

export async function loadConfig(storage = chrome.storage.local) {
  const { port, token } = await storage.get(["port", "token"]);
  return { port, token };
}

export async function captureSelection(info, deps = {}) {
  const fetchImpl = deps.fetch ?? fetch;
  const config = deps.config ?? (await loadConfig());
  if (!config.token) return { ok: false, message: "Not paired. Open the Kakurega popup to pair." };
  const text = cleanSelection(info?.selectionText);
  if (!text) return { ok: false, message: "That selection cannot be sent: it is empty, too long or has control characters." };
  const page = pageOf(info?.pageUrl);
  if (!page) return { ok: false, message: "Only pages on http or https can be sent." };
  try {
    const r = await call(fetchImpl, config, "POST", "/ext/capture", { text, page });
    if (r.status === 202) return { ok: true, message: "Queued in Kakurega. Review it there before anything is saved." };
    return { ok: false, message: describeFailure(r.status, r.body) };
  } catch {
    return { ok: false, message: "Kakurega is not running on this computer." };
  }
}
