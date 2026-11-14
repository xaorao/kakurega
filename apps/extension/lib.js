export const MAX_SELECTION = 2000;

export function parsePort(value) {
  const text = String(value ?? "").trim();
  if (!/^[0-9]{1,5}$/.test(text)) return null;
  const n = Number(text);
  return n >= 1024 && n <= 65535 ? n : null;
}

export function baseUrl(port) {
  const p = parsePort(port);
  return p === null ? null : `http://127.0.0.1:${p}`;
}

export function cleanSelection(text) {
  if (typeof text !== "string") return null;
  const t = text.replace(/\r\n?/g, "\n").trim();
  if (t.length === 0 || [...t].length > MAX_SELECTION) return null;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f‪-‮⁦-⁩]/.test(t)) return null;
  return t;
}

/** Why a selection cannot be sent, as a sentence, or null when it can. The same rules as `cleanSelection`. */
export function selectionProblem(text) {
  if (typeof text !== "string" || text.trim().length === 0) return "Nothing is selected. Select some text on the page first.";
  const t = text.replace(/\r\n?/g, "\n").trim();
  const n = [...t].length;
  if (n > MAX_SELECTION) return `That selection is too long to send: it has ${n} characters and the most is ${MAX_SELECTION}. Select a shorter part.`;
  if (cleanSelection(text) === null) return "That selection holds hidden characters (control characters or marks that change the direction of text), often from copying. It was not sent.";
  return null;
}

export function pageOf(url) {
  try {
    const u = new URL(url);
    return u.protocol === "https:" || u.protocol === "http:" ? `${u.protocol}//${u.host}/` : null;
  } catch {
    return null;
  }
}

export function describeFailure(status, body) {
  if (status === 401) return "Not paired. Open the Kakurega popup and pair again.";
  if (status === 422) return sentence(body?.error?.message ?? "Not queued.");
  if (status === 429) return "Too many wrong codes. Wait a minute.";
  return sentence(body?.error?.message ?? `The service answered ${status}.`);
}

/** What the service said, as a sentence: it writes its reasons to follow other words, without a capital or a full stop. */
function sentence(text) {
  const t = String(text).trim();
  const first = /^[a-z]/.test(t) ? t.charAt(0).toUpperCase() + t.slice(1) : t;
  return /[.!?]$/.test(first) ? first : first + ".";
}

export async function call(fetchImpl, { port, token }, method, path, body) {
  const base = baseUrl(port);
  if (!base) throw new Error("No valid port is saved.");
  const headers = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const res = await fetchImpl(`${base}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), cache: "no-store", credentials: "omit", redirect: "error" });
  let json = null;
  try {
    json = await res.json();
  } catch {
    json = null;
  }
  return { status: res.status, body: json };
}
