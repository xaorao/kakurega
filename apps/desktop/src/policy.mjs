export function isAppUrl(url, origin) {
  try {
    return new URL(url).origin === origin;
  } catch {
    return false;
  }
}

export function allowPermission() {
  return false;
}

export function mayOpenExternally(url) {
  try {
    const u = new URL(url);
    return u.protocol === "https:" && u.username === "" && u.password === "";
  } catch {
    return false;
  }
}

export function windowOptions() {
  return {
    width: 1180,
    height: 820,
    minWidth: 760,
    minHeight: 560,
    show: false,
    backgroundColor: "#ffffff",
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
      allowRunningInsecureContent: false,
      webviewTag: false,
      spellcheck: false,
    },
  };
}

export const SHELL_HEADER = "x-kk-shell";

/** A fresh secret for one launch of the companion: 256 random bits, text safe in a header. */
export function newShellSecret(randomBytes) {
  return randomBytes(32).toString("base64url");
}

/**
 * The headers of one outgoing request, with the shell secret added only when the request goes to the companion's own origin.
 * Any other address, and any lookalike of the origin, gets the headers untouched, so the secret cannot reach another server.
 */
export function withShellSecret(url, headers, origin, secret) {
  if (!secret || !isAppUrl(url, origin)) return headers;
  const out = {};
  for (const [k, v] of Object.entries(headers)) if (k.toLowerCase() !== SHELL_HEADER) out[k] = v;
  out[SHELL_HEADER] = secret;
  return out;
}
