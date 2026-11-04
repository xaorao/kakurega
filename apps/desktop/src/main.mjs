import { app, BrowserWindow, dialog, shell, systemPreferences, session } from "electron";
import { spawn, execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { join } from "node:path";
import { homedir } from "node:os";
import { readAdmin, adminClient } from "./admin.mjs";
import { createApprover } from "./approval.mjs";
import { isAppUrl, allowPermission, mayOpenExternally, windowOptions } from "./policy.mjs";

const dataDir = process.env.KAKUREGA_DATA_DIR ?? join(homedir(), ".kakurega");
const kakuBin = process.env.KAKU_BIN;
let child = null;
let window = null;

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
      const r = await fetch(admin.base + "/api/session");
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

async function start() {
  if (kakuBin) {
    child = spawn(kakuBin, ["serve", "--data-dir", dataDir, ...(process.env.KAKU_ARGS ? process.env.KAKU_ARGS.split(" ") : [])], { stdio: "ignore" });
    child.on("exit", () => {
      if (!app.isQuitting) dialog.showErrorBox("Kakurega stopped", "The local companion exited. Restart the app.");
    });
  }
  const admin = await waitForAdmin();
  const client = adminClient(admin);
  const pairing = await client.post("/admin/pairing");
  if (pairing.status !== 200) throw new Error("could not get a pairing link");

  session.defaultSession.setPermissionRequestHandler((_wc, _perm, cb) => cb(allowPermission()));
  session.defaultSession.setPermissionCheckHandler(() => allowPermission());

  window = new BrowserWindow(windowOptions());
  window.webContents.setWindowOpenHandler(({ url }) => {
    if (mayOpenExternally(url)) shell.openExternal(url);
    return { action: "deny" };
  });
  window.webContents.on("will-navigate", (event, url) => {
    if (isAppUrl(url, admin.origin)) return;
    event.preventDefault();
    if (mayOpenExternally(url)) shell.openExternal(url);
  });
  window.webContents.on("will-frame-navigate", (event) => {
    if (!isAppUrl(event.url, admin.origin)) event.preventDefault();
  });
  // will-navigate and will-frame-navigate do not fire for every target (about:blank slips through), so any
  // top-level page that is not the app is replaced at once.
  window.webContents.on("did-navigate", (_e, url) => {
    if (!isAppUrl(url, admin.origin)) window.loadURL(`${admin.origin}/`).catch(() => {});
  });
  window.webContents.on("will-attach-webview", (event) => event.preventDefault());
  window.once("ready-to-show", () => window.show());
  await window.loadURL(`${admin.origin}/#pair=${pairing.json.code}`);

  const signer = (id) => {
    const f = enclaveSigner() ?? softwareSigner();
    return f ? f(id) : Promise.reject(new Error("no approver key is enrolled on this computer"));
  };
  Object.defineProperty(signer, "ownsDialog", { get: () => enclaveSigner() !== null });
  const pollOnce = createApprover({ client, confirm, authenticate, sign: signer });
  setInterval(() => pollOnce().catch(() => {}), 1000);
}

app.whenReady().then(start).catch((e) => {
  dialog.showErrorBox("Kakurega could not start", String(e.message ?? e));
  app.quit();
});
app.on("before-quit", () => {
  app.isQuitting = true;
  child?.kill();
});
app.on("window-all-closed", () => app.quit());
