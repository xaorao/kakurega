import { app, BrowserWindow, dialog, shell, systemPreferences, session, Notification } from "electron";
import { spawn, execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { join } from "node:path";
import { homedir } from "node:os";
import { readAdmin, adminClient } from "./admin.mjs";
import { createApprover } from "./approval.mjs";
import { randomBytes } from "node:crypto";
import { isAppUrl, allowPermission, mayOpenExternally, windowOptions, newShellSecret, withShellSecret, SHELL_HEADER } from "./policy.mjs";
import { createRestartPolicy, rendererCrashed } from "./recovery.mjs";

const dataDir = process.env.KAKUREGA_DATA_DIR ?? join(homedir(), ".kakurega");
const kakuBin = process.env.KAKU_BIN;
// When the data folder is chosen explicitly (a second profile, a test), the window's own files (cache, cookies) live inside it
// instead of in the one place every copy of the app shares, so copies cannot damage each other's caches.
if (process.env.KAKUREGA_DATA_DIR) app.setPath("userData", join(dataDir, "electron"));
// `app.isPackaged` is false for a bundle whose program is still called Electron, which is how this one is built, so the test for "this is
// the packaged app" is that its code is read from an archive
const runsFromAsar = app.getAppPath().endsWith(".asar");
let child = null;
let window = null;
// The companion listens on a new port every time it starts, so everything that talks to it reads the current one.
let current = null;
// Chosen anew for every companion this shell starts and given to it on stdin. Only this window's requests carry it, so no
// other program on the computer can load the app or try its pairing door.
let shellSecret = null;
const companionPolicy = createRestartPolicy();
const rendererPolicy = createRestartPolicy({ baseDelayMs: 0 });

function tell(title, body) {
  try {
    if (Notification.isSupported()) new Notification({ title, body }).show();
  } catch {}
}

function enclaveSigner() {
  const blob = join(dataDir, "approver.enclave");
  const helper = process.env.KAKU_ENCLAVE_BIN ?? (kakuBin ? join(kakuBin, "..", "kaku-enclave") : null);
  if (!existsSync(blob) || !helper || !existsSync(helper)) return null;
  return (approvalId) =>
    new Promise((resolve, reject) => {
      execFile(helper, ["approve", "--blob", blob, "--data-dir", dataDir, "--approval", approvalId], { timeout: 300000, maxBuffer: 65536 }, (err, out) => (err ? reject(err) : resolve(String(out).trim())));
    });
}

// A software key kept by the kaku binary (never read by this process). The owner confirms in this shell's own dialog
// first; kaku fetches the release from the kernel itself and signs only that.
function softwareSigner() {
  if (!kakuBin || !existsSync(join(dataDir, "approver.key"))) return null;
  return (approvalId) =>
    new Promise((resolve, reject) => {
      execFile(kakuBin, ["approver", "sign", "--approval", approvalId, "--data-dir", dataDir], { timeout: 60000, maxBuffer: 65536 }, (err, out) => (err ? reject(err) : resolve(String(out).trim())));
    });
}

async function waitForAdmin() {
  for (let i = 0; i < 100; i++) {
    try {
      const admin = await readAdmin(dataDir);
      const r = await fetch(admin.base + "/api/session", { headers: shellSecret ? { [SHELL_HEADER]: shellSecret } : {} });
      if (r.ok) return admin;
    } catch {}
    await sleep(100);
  }
  throw new Error("the Kakurega companion did not start");
}

async function authenticate(reason) {
  if (process.platform !== "darwin" || !systemPreferences.canPromptTouchID()) return false;
  try {
    await systemPreferences.promptTouchID(reason);
    return true;
  } catch {
    return false;
  }
}

async function confirm(text) {
  const r = await dialog.showMessageBox({
    type: "question",
    title: "Approve release",
    message: "Approve this release?",
    detail: text,
    buttons: ["Refuse", "Approve with Touch ID"],
    defaultId: 0,
    cancelId: 0,
    noLink: true,
  });
  return r.response === 1;
}

async function connect() {
  const admin = await waitForAdmin();
  const client = adminClient(admin);
  const pairing = await client.post("/admin/pairing");
  if (pairing.status !== 200) throw new Error("could not get a pairing link");
  current = { admin, client, code: pairing.json.code };
}

function launchCompanion() {
  shellSecret = newShellSecret(randomBytes);
  child = spawn(kakuBin, ["serve", "--data-dir", dataDir, "--shell-secret-stdin", ...(runsFromAsar ? ["--require-approver"] : []), ...(process.env.KAKU_ARGS ? process.env.KAKU_ARGS.split(" ") : [])], { stdio: ["pipe", "ignore", "ignore"] });
  child.stdin.on("error", () => {});
  // the pipe stays open: when this process goes away, however it goes, the companion sees it close and leaves too
  child.stdin.write(shellSecret + "\n");
  const mine = child;
  mine.on("exit", () => {
    if (app.isQuitting || child !== mine) return;
    const d = companionPolicy.next();
    if (!d.restart) {
      dialog.showErrorBox("Kakurega stopped", "The local companion keeps stopping, so it was not started again. Your data is safe. Quit and open Kakurega again; if it keeps happening, check the activity log.");
      return;
    }
    setTimeout(async () => {
      try {
        launchCompanion();
        await connect();
        await window?.loadURL(`${current.admin.origin}/#pair=${current.code}`);
        tell("Kakurega restarted", "The local companion stopped and was started again. Anything that was running at that moment was stopped; nothing was sent anywhere.");
      } catch (e) {
        dialog.showErrorBox("Kakurega stopped", `The local companion could not be restarted: ${e.message ?? e}`);
      }
    }, d.delayMs);
  });
}

async function start() {
  if (kakuBin) launchCompanion();
  await connect();
  const { admin, code } = current;
  session.defaultSession.setPermissionRequestHandler((_wc, _perm, cb) => cb(allowPermission()));
  session.defaultSession.setPermissionCheckHandler(() => allowPermission());
  session.defaultSession.setDevicePermissionHandler(() => false);

  session.defaultSession.webRequest.onBeforeSendHeaders({ urls: ["http://127.0.0.1/*"] }, (details, callback) => {
    callback({ requestHeaders: withShellSecret(details.url, details.requestHeaders, current.admin.origin, shellSecret) });
  });
  window = new BrowserWindow(windowOptions());
  window.webContents.setWindowOpenHandler(({ url }) => {
    if (mayOpenExternally(url)) shell.openExternal(url);
    return { action: "deny" };
  });
  window.webContents.on("will-navigate", (event, url) => {
    if (isAppUrl(url, current.admin.origin)) return;
    event.preventDefault();
    if (mayOpenExternally(url)) shell.openExternal(url);
  });
  window.webContents.on("will-redirect", (event, url) => {
    if (!isAppUrl(url, current.admin.origin)) event.preventDefault();
  });
  window.webContents.on("will-frame-navigate", (event) => {
    if (!isAppUrl(event.url, current.admin.origin)) event.preventDefault();
  });
  // will-navigate and will-frame-navigate do not fire for every target (about:blank slips through), so any
  // top-level page that is not the app is replaced at once.
  window.webContents.on("did-navigate", (_e, url) => {
    if (!isAppUrl(url, current.admin.origin)) window.loadURL(`${current.admin.origin}/`).catch(() => {});
  });
  window.webContents.on("will-attach-webview", (event) => event.preventDefault());
  window.once("ready-to-show", () => window.show());
  window.webContents.on("render-process-gone", (_e, details) => {
    if (!rendererCrashed(details) || app.isQuitting) return;
    const d = rendererPolicy.next();
    if (!d.restart) {
      dialog.showErrorBox("Kakurega window stopped", "The window keeps crashing, so it was not reloaded again. Quit and open Kakurega again.");
      return;
    }
    window.webContents.reload();
    tell("Kakurega reloaded the window", "The window crashed and was reloaded. Your data and any running reviews were not affected.");
  });
  let asking = false;
  window.on("unresponsive", async () => {
    if (asking || app.isQuitting) return;
    asking = true;
    const r = await dialog.showMessageBox(window, { type: "warning", title: "Kakurega is not responding", message: "The window is not responding.", buttons: ["Wait", "Reload the window"], defaultId: 0, cancelId: 0 });
    asking = false;
    if (r.response === 1) window.webContents.reload();
  });
  await window.loadURL(`${admin.origin}/#pair=${code}`);

  const signer = (id) => {
    const f = enclaveSigner() ?? softwareSigner();
    return f ? f(id) : Promise.reject(new Error("no approver key is enrolled on this computer"));
  };
  Object.defineProperty(signer, "ownsDialog", { get: () => enclaveSigner() !== null });
  const pollOnce = createApprover({ client: { get: (p) => current.client.get(p), post: (p, b) => current.client.post(p, b) }, confirm, authenticate, sign: signer });
  setInterval(
    () =>
      pollOnce()
        .then((done) => {
          if (done.some((d) => d.outcome === "needs_enrolled_key")) tell("A release is waiting, but approving needs a signing key", "Run `kaku approver enroll` once in a terminal, then approve the release there with `kaku approve`.");
        })
        .catch(() => {}),
    1000,
  );
}

// One window, one companion: a second launch only brings the first one forward.
if (!app.requestSingleInstanceLock()) {
  app.isQuitting = true;
  app.quit();
} else {
  app.on("second-instance", () => {
    if (window) {
      if (window.isMinimized()) window.restore();
      window.focus();
    }
  });
}

app.whenReady().then(() => (app.isQuitting ? undefined : start())).catch((e) => {
  dialog.showErrorBox("Kakurega could not start", String(e.message ?? e));
  app.quit();
});
app.on("before-quit", () => {
  app.isQuitting = true;
  const c = child;
  child = null;
  c?.kill();
});
app.on("window-all-closed", () => app.quit());
