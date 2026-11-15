import { app, BrowserWindow, dialog, shell, systemPreferences, session, Notification, Tray, Menu, nativeImage, desktopCapturer, screen, globalShortcut } from "electron";
import { spawn, execFile } from "node:child_process";
import { existsSync, readFileSync, writeFileSync, mkdirSync, rmSync, mkdtempSync, chmodSync, readdirSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { join } from "node:path";
import { homedir } from "node:os";
import { readAdmin, adminClient } from "./admin.mjs";
import { createApprover } from "./approval.mjs";
import { randomBytes } from "node:crypto";
import { isAppUrl, allowPermission, mayOpenExternally, windowOptions, newShellSecret, withShellSecret, SHELL_HEADER } from "./policy.mjs";
import { createRestartPolicy, rendererCrashed } from "./recovery.mjs";
import { english, pickLanguage, translator } from "./strings.mjs";
import { createNotifier, readInbox } from "./inbox.mjs";
import { menuTemplate, trayIconPng, trayTitle, trayToolTip } from "./tray.mjs";
import { KAKU_HELPERS, bundledPaths, checkPart, readManifest } from "./bundle.mjs";
import { createCapture, pickSource, MAX_PICTURE_BYTES } from "./capture.mjs";
import { chooseProfile, currentProfile, profileDir, profileNames, DEFAULT as DEFAULT_PROFILE } from "./profiles.mjs";

// After a crash or a forced quit macOS asks, in a box of its own, whether to reopen the app's windows, and holds the app
// until someone answers: nothing of ours runs meanwhile, so the service is not started and the tray is not there. This
// app restores its own state (the service keeps everything), so macOS is told not to keep or restore window state.
if (process.platform === "darwin") {
  try {
    systemPreferences.setUserDefault("ApplePersistenceIgnoreState", "boolean", true);
    systemPreferences.setUserDefault("NSQuitAlwaysKeepsWindows", "boolean", false);
  } catch {
    // a default that cannot be written leaves the system's own behaviour
  }
}

// The data folder: the one named for a test or a special set-up, else the profile in use (see profiles.mjs).
const profilesHome = process.env.KAKUREGA_HOME ?? join(homedir(), ".kakurega");
const profileName = process.env.KAKUREGA_DATA_DIR ? null : currentProfile(profilesHome);
const dataDir = process.env.KAKUREGA_DATA_DIR ?? profileDir(profilesHome, profileName);
// When the data folder is chosen explicitly (a second profile, a test), the window's own files (cache, cookies) live inside it
// instead of in the one place every copy of the app shares, so copies cannot damage each other's caches.
// a named profile keeps the shell's own state (window settings, the page's storage) inside its own folder too
// the name people see in the menu bar, the Dock and "About": the package name is not one
app.setName("Kakurega");
// The only thing this window keeps is the cookie of its session with the service on this computer, which is useless
// without the secret this process holds in memory. It is not worth a keychain item: an unsigned or re-signed build
// would be asked for the login keychain password at every start, and could hang before its window shows.
app.commandLine.appendSwitch("use-mock-keychain");
if (process.env.KAKUREGA_DATA_DIR || profileName !== DEFAULT_PROFILE) app.setPath("userData", join(dataDir, "electron"));
// `app.isPackaged` is false for a bundle whose program is still called Electron, which is how this one is built, so the test for "this is
// the packaged app" is that its code is read from an archive
const runsFromAsar = app.getAppPath().endsWith(".asar");
// A packaged macOS app carries its own service, web interface and signing helper, with their values in bundle.json inside the
// asar. KAKU_BIN still points the app at another kaku, for development.
let manifest = null;
let manifestError = null;
try {
  manifest = runsFromAsar ? readManifest(app.getAppPath()) : null;
} catch (e) {
  manifestError = e;
}
const bundled = manifest ? bundledPaths(process.resourcesPath) : null;
const kakuFromBundle = !process.env.KAKU_BIN && !!bundled;
const kakuBin = process.env.KAKU_BIN ?? bundled?.kaku;
const extraArgs = process.env.KAKU_ARGS ? process.env.KAKU_ARGS.split(" ") : [];
const webFromBundle = !!bundled && !extraArgs.includes("--ui-dir");
const pollMs = Math.max(250, Number.parseInt(process.env.KAKUREGA_INBOX_POLL_MS ?? "", 10) || 2000);
const settingsPath = () => join(app.getPath("userData"), "shell.json");
const macOS = process.platform === "darwin";

let child = null;
let window = null;
let tray = null;
let trayMenu = null;
let trayKey = "";
// The companion listens on a new port every time it starts, so everything that talks to it reads the current one.
let current = null;
// the address of the service this window was last given: while the service restarts there is no connection, and the
// window's own pages are still told apart from everything else by it
let knownOrigin = "";
const appOrigin = () => current?.admin.origin ?? knownOrigin;
// Chosen anew for every companion this shell starts and given to it on stdin. Only this window's requests carry it, so no
// other program on the computer can load the app or try its pairing door.
let shellSecret = null;
let t = english;
let language = "en";
const companionPolicy = createRestartPolicy();
const rendererPolicy = createRestartPolicy({ baseDelayMs: 0 });
const state = { count: 0, stopped: false, notify: true, keyAnywhere: true, keysText: process.platform === "darwin" ? "⇧⌥⌘K" : "Ctrl+Alt+Shift+K", complete: true };
// The notifications this run has shown, newest last, so a click can still be handled; only generic text is ever in them.
const notices = [];

function tell(title, body) {
  try {
    if (Notification.isSupported()) new Notification({ title, body }).show();
  } catch {}
}

/** Which part of the bundle differs from bundle.json, or null. Only the parts this run will use are checked. */
function changedPart() {
  if (manifestError) return "bundle.json";
  if (!manifest) return null;
  if (kakuFromBundle) {
    const c = checkPart(manifest, "kaku", bundled.kaku);
    if (c) return c;
    // kaku starts its helpers from beside itself: each must be the one packaging recorded, and one it did not record must
    // not be there at all
    for (const helper of KAKU_HELPERS) {
      if (manifest[helper]) {
        const h = checkPart(manifest, helper, bundled[helper]);
        if (h) return h;
      } else if (existsSync(bundled[helper])) {
        return helper;
      }
    }
  }
  if (webFromBundle) return checkPart(manifest, "web", bundled.web);
  return null;
}

function refuseChanged(what) {
  console.error(`kakurega: not started: ${what} does not match bundle.json`);
  // the part is named in words; its short name in the bundle list means nothing to the person reading
  const key = `part.${what}`;
  const named = t(key);
  dialog.showErrorBox(t("error.changed.title"), t("error.changed.body", { what: named === key ? what : named }));
}

function enclaveHelper() {
  if (process.env.KAKU_ENCLAVE_BIN) return { path: process.env.KAKU_ENCLAVE_BIN, checked: false };
  if (!kakuBin) return null;
  const path = join(kakuBin, "..", "kaku-enclave");
  // a helper beside the bundled kaku is used only if packaging put it there and recorded its value
  if (kakuFromBundle) return manifest.enclave ? { path, checked: true } : null;
  return { path, checked: false };
}

function enclaveSigner() {
  const blob = join(dataDir, "approver.enclave");
  const helper = enclaveHelper();
  if (!existsSync(blob) || !helper || !existsSync(helper.path)) return null;
  return (approvalId) =>
    new Promise((resolve, reject) => {
      if (helper.checked && checkPart(manifest, "enclave", helper.path)) return reject(new Error("the signing helper does not match bundle.json"));
      execFile(helper.path, ["approve", "--blob", blob, "--data-dir", dataDir, "--approval", approvalId], { timeout: 300000, maxBuffer: 65536 }, (err, out) => (err ? reject(err) : resolve(String(out).trim())));
    });
}

// A software key kept by the kaku binary (never read by this process). The owner confirms in this shell's own dialog
// first; kaku fetches the release from the kernel itself and signs only that.
function softwareSigner() {
  if (!kakuBin || !existsSync(join(dataDir, "approver.key"))) return null;
  return (approvalId) =>
    new Promise((resolve, reject) => {
      if (kakuFromBundle && checkPart(manifest, "kaku", kakuBin)) return reject(new Error("kaku does not match bundle.json"));
      execFile(kakuBin, ["approver", "sign", "--approval", approvalId, "--data-dir", dataDir], { timeout: 60000, maxBuffer: 65536 }, (err, out) => (err ? reject(err) : resolve(String(out).trim())));
    });
}

// A packaged app only takes approvals signed by a key enrolled on this computer. Nobody who downloaded an app has a
// terminal command at hand, so the app offers to make that key itself: in the Secure Enclave where the Mac has Touch ID,
// else a file private to this user. kaku makes and enrols it; this process never reads the key.
let enrolling = false;
// approvals already put to the owner, and those that could not be because no key existed yet
const askedApprovals = new Set();
const waitingForKey = new Set();
function runKaku(args, timeout = 120000) {
  return new Promise((resolve) => {
    if (!kakuBin) return resolve({ ok: false, said: "kaku was not found" });
    if (kakuFromBundle && checkPart(manifest, "kaku", kakuBin)) return resolve({ ok: false, said: "kaku does not match bundle.json" });
    execFile(kakuBin, args, { timeout, maxBuffer: 65536 }, (err, out, errOut) => resolve({ ok: !err, said: String(err ? errOut || err.message : out).trim() }));
  });
}
async function offerEnrol() {
  if (enrolling) return false;
  enrolling = true;
  try {
    const helper = enclaveHelper();
    const hardware = hasTouchID() && helper !== null && existsSync(helper.path) && !(helper.checked && checkPart(manifest, "enclave", helper.path));
    const options = {
      type: "question",
      message: t("enrol.message"),
      detail: t(hardware ? "enrol.detail.enclave" : "enrol.detail.file"),
      buttons: [t("enrol.later"), t("enrol.yes")],
      defaultId: 1,
      cancelId: 0,
      noLink: true,
    };
    const onWindow = window && !window.isDestroyed();
    if (onWindow) showWindow();
    const r = onWindow ? await dialog.showMessageBox(window, options) : await dialog.showMessageBox(options);
    if (r.response !== 1) return false;
    const base = ["approver", "enroll", "--data-dir", dataDir, "--label", t("enrol.label")];
    let done = hardware ? await runKaku([...base, "--enclave"]) : { ok: false, said: "" };
    // the Secure Enclave can refuse (no passcode set, a managed Mac): the file key still lets the owner approve
    const fellBack = hardware && !done.ok;
    if (!done.ok) done = await runKaku(base);
    if (!done.ok) {
      dialog.showErrorBox(t("enrol.failed.title"), t("enrol.failed.body", { why: done.said.slice(0, 300) }));
      return false;
    }
    // what waited for a key is put to the owner at the next look
    for (const id of waitingForKey) askedApprovals.delete(id);
    waitingForKey.clear();
    tell(t("enrol.done.title"), t(fellBack || !hardware ? "enrol.done.file" : "enrol.done.enclave"));
    return true;
  } finally {
    enrolling = false;
  }
}

async function waitForAdmin() {
  let other = null;
  for (let i = 0; i < 100; i++) {
    try {
      const admin = await readAdmin(dataDir);
      // only the service this app started: an admin.json left by a service that crashed, or written by one started from a
      // terminal on the same data folder, names another process, and its port may now be anyone's
      if (child && admin.pid === child.pid) {
        const r = await fetch(admin.base + "/api/session", { headers: shellSecret ? { [SHELL_HEADER]: shellSecret } : {} });
        if (r.ok) return admin;
      } else if (admin.pid !== null && admin.pid !== child?.pid) {
        other = admin.pid;
      }
    } catch {}
    if (child && child.exitCode !== null) break;
    await sleep(100);
  }
  if (other !== null && processAlive(other)) throw new Error(t("error.otherService", { pid: other }));
  throw new Error("the Kakurega companion did not start");
}

function processAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e?.code === "EPERM";
  }
}

/** Whether this computer can ask for a fingerprint at all. */
const hasTouchID = () => macOS && systemPreferences.canPromptTouchID();

async function authenticate(reason) {
  if (!hasTouchID()) return "unavailable";
  try {
    await systemPreferences.promptTouchID(reason);
    return true;
  } catch {
    return false;
  }
}

async function confirm(text, a) {
  // the button promises a fingerprint only when one will be asked for: not on a computer without Touch ID, and not when
  // an enrolled approver key signs instead
  const touch = hasTouchID() && !a?.requires_proof;
  const options = {
    type: "question",
    title: t("approve.title"),
    message: t("approve.message"),
    detail: touch || a?.requires_proof ? text : `${t("approve.noTouch")}\n\n${text}`,
    buttons: [t("approve.refuse"), t(touch ? "approve.yes" : "approve.yesPlain")],
    defaultId: 0,
    cancelId: 0,
    noLink: true,
  };
  // attached to the window when there is one: the rest of the app (menu bar, quit) keeps working while it is open
  const onWindow = window && !window.isDestroyed();
  if (onWindow) {
    if (window.isMinimized()) window.restore();
    window.show();
  }
  const r = onWindow ? await dialog.showMessageBox(window, options) : await dialog.showMessageBox(options);
  return r.response === 1;
}

/** The application menu: what a Mac app is expected to have, and developer tools only when run from source. */
function appMenu() {
  const view = [{ role: "reload" }, { type: "separator" }, { role: "resetZoom" }, { role: "zoomIn" }, { role: "zoomOut" }, { type: "separator" }, { role: "togglefullscreen" }];
  if (!runsFromAsar) view.push({ type: "separator" }, { role: "toggleDevTools" });
  return [
    ...(macOS ? [{ label: "Kakurega", submenu: [{ role: "about" }, { type: "separator" }, { label: t("menu.settings"), accelerator: "CmdOrCtrl+,", click: () => { if (current && window && !window.isDestroyed()) { showWindow(); void window.loadURL(`${current.admin.origin}/#/settings`); } } }, { label: t("menu.enrol"), click: () => void offerEnrol() }, { type: "separator" }, { role: "hide" }, { role: "hideOthers" }, { role: "unhide" }, { type: "separator" }, { role: "quit" }] }] : []),
    { label: t("menu.file"), submenu: [{ role: "close" }] },
    { role: "editMenu" },
    { label: "View", submenu: view },
    { role: "windowMenu" },
    {
      role: "help",
      submenu: [
        { label: t("menu.help.limits"), click: () => void shell.openExternal("https://github.com/xaorao/kakurega/blob/main/docs/LIMITS.md") },
        { label: t("menu.help.source"), click: () => void shell.openExternal("https://github.com/xaorao/kakurega") },
      ],
    },
  ];
}

// The window's session ended while the service is the same one (Sign out was pressed in the window): the desktop app
// pairs its own window again, since a person here has no pairing link to type.
let repairing = false;
async function pairAgain() {
  if (repairing || !current || state.stopped || !window || window.isDestroyed()) return;
  repairing = true;
  try {
    const pairing = await current.client.post("/admin/pairing");
    if (pairing.status === 200 && current) {
      current.code = pairing.json.code;
      await window.loadURL(`${current.admin.origin}/#pair=${current.code}`);
      window.webContents.reload();
    }
  } catch {
    /* the service went away meanwhile: the restart path pairs the window */
  } finally {
    setTimeout(() => { repairing = false; }, 3000);
  }
}

async function connect() {
  const admin = await waitForAdmin();
  const client = adminClient(admin);
  const pairing = await client.post("/admin/pairing");
  if (pairing.status !== 200) throw new Error("could not get a pairing link");
  current = { admin, client, code: pairing.json.code };
  knownOrigin = admin.origin;
}

function launchCompanion() {
  shellSecret = newShellSecret(randomBytes);
  const args = ["serve", "--data-dir", dataDir, "--shell-secret-stdin", ...(runsFromAsar ? ["--require-approver"] : []), ...(webFromBundle ? ["--ui-dir", bundled.web] : []), ...extraArgs];
  child = spawn(kakuBin, args, { stdio: ["pipe", "ignore", "ignore"] });
  child.stdin.on("error", () => {});
  // the pipe stays open: when this process goes away, however it goes, the companion sees it close and leaves too
  child.stdin.write(shellSecret + "\n");
  const mine = child;
  mine.on("exit", () => {
    if (app.isQuitting || child !== mine) return;
    // from now until a new service is connected nothing goes to the old address: not a picture, not the key
    state.stopped = true;
    current = null;
    syncShortcut();
    refreshTray();
    const d = companionPolicy.next();
    if (!d.restart) {
      state.stopped = true;
      refreshTray();
      dialog.showErrorBox(t("error.stopped.title"), t("error.stopped.loop"));
      return;
    }
    setTimeout(async () => {
      try {
        // a restart runs the file that is there now, so it is checked again
        const changed = changedPart();
        if (changed) {
          state.stopped = true;
          refreshTray();
          refuseChanged(changed);
          return;
        }
        launchCompanion();
        await connect();
        state.stopped = false;
        syncShortcut();
        refreshTray();
        // the new service usually has the address the old one had, and a link that differs only after the # does not
        // load the page again: the page is loaded anew so it pairs with the new service
        const before = window?.webContents.getURL() ?? "";
        await window?.loadURL(`${current.admin.origin}/#pair=${current.code}`);
        if (before.startsWith(`${current.admin.origin}/`)) window?.webContents.reload();
        tell(t("notice.restarted.title"), t("notice.restarted.body"));
      } catch (e) {
        dialog.showErrorBox(t("error.stopped.title"), t("error.stopped.restart", { why: e.message ?? e }));
      }
    }, d.delayMs);
  });
}

function readSettings() {
  try {
    const s = JSON.parse(readFileSync(settingsPath(), "utf8"));
    return { notify: s?.notify !== false, keyAnywhere: s?.keyAnywhere !== false };
  } catch {
    return { notify: true, keyAnywhere: true };
  }
}

function saveSettings() {
  try {
    mkdirSync(app.getPath("userData"), { recursive: true });
    writeFileSync(settingsPath(), JSON.stringify({ notify: state.notify, keyAnywhere: state.keyAnywhere }), { mode: 0o600 });
  } catch {}
}

/** Brings the window forward, optionally on one screen of the app (`#/inbox`, `#/job/…`, `#/team/…`). */
function showWindow(hash) {
  if (!window || window.isDestroyed()) return;
  if (hash && current && /^#\/[A-Za-z0-9_/-]{0,80}$/.test(hash)) {
    const target = `${current.admin.origin}/${hash}`;
    if (window.webContents.getURL() !== target) window.loadURL(target).catch(() => {});
  }
  if (window.isMinimized()) window.restore();
  window.show();
  window.focus();
}

// A picture of the screen for the chat. Taken here, in the main process, only from the shell's own menu; the page has no
// way to ask for it and its permission requests stay refused.
let captureTestPng = null;
let captureTestAnswer = null;

async function grabScreen() {
  if (captureTestPng) return captureTestPng;
  if (macOS && systemPreferences.getMediaAccessStatus("screen") === "denied") return "denied";
  const display = window && !window.isDestroyed() ? screen.getDisplayMatching(window.getBounds()) : screen.getPrimaryDisplay();
  // the app's own window is taken out of the picture: the owner wants what is behind it
  const wasVisible = !!window && !window.isDestroyed() && window.isVisible();
  if (wasVisible) {
    window.hide();
    await sleep(350);
  }
  try {
    const sources = await desktopCapturer.getSources({
      types: ["screen"],
      thumbnailSize: { width: Math.round(display.size.width * display.scaleFactor), height: Math.round(display.size.height * display.scaleFactor) },
    });
    const source = pickSource(sources, display.id);
    if (!source || source.thumbnail.isEmpty()) return macOS && systemPreferences.getMediaAccessStatus("screen") !== "granted" ? "denied" : null;
    let png = source.thumbnail.toPNG();
    // a full-resolution picture of a large screen can pass the chat's file limit: the picture is then made at the screen's
    // size in points, which is still readable
    if (png.length > MAX_PICTURE_BYTES) png = source.thumbnail.resize({ width: display.size.width, quality: "best" }).toPNG();
    return png;
  } finally {
    if (wasVisible && window && !window.isDestroyed()) window.show();
  }
}

/**
 * Opens the picture in a window that runs no script and has no bridge. The picture is put in a folder of its own that
 * only this user can read and shown from there: a picture of a large screen is several megabytes, more than a page
 * address may hold, so it cannot be handed to the window inside an address. `close()` removes the window and the folder.
 */
async function openPreview(png) {
  const image = nativeImage.createFromBuffer(png);
  const { width, height } = image.getSize();
  const area = screen.getPrimaryDisplay().workAreaSize;
  const scale = Math.min(1, (area.width * 0.6) / Math.max(1, width), (area.height * 0.6) / Math.max(1, height));
  const dir = mkdtempSync(join(app.getPath("temp"), "kakurega-preview-"));
  chmodSync(dir, 0o700);
  writeFileSync(join(dir, "shot.png"), png, { mode: 0o600 });
  const title = t("capture.preview.title").replace(/[<>&]/g, "");
  writeFileSync(join(dir, "preview.html"), `<!doctype html><meta charset="utf-8"><title>${title}</title><body style="margin:0;background:#111"><img alt="" style="display:block;width:100vw;height:100vh;object-fit:contain" src="shot.png">`, { mode: 0o600 });
  const preview = new BrowserWindow({
    width: Math.max(320, Math.round(width * scale)),
    height: Math.max(200, Math.round(height * scale)),
    title: t("capture.preview.title"),
    resizable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    backgroundColor: "#111111",
    webPreferences: { javascript: false, sandbox: true, contextIsolation: true, nodeIntegration: false, partition: "kakurega-capture-preview" },
  });
  preview.setMenuBarVisibility(false);
  preview.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  preview.webContents.on("will-navigate", (e) => e.preventDefault());
  const close = () => {
    if (!preview.isDestroyed()) preview.destroy();
    rmSync(dir, { recursive: true, force: true });
  };
  try {
    await preview.loadFile(join(dir, "preview.html"));
  } catch (e) {
    close();
    throw e;
  }
  return { preview, close };
}

/** Shows the picture and asks with a native dialog on its window. */
let captureRelease = null;
async function confirmPicture(png) {
  // a test may hold the answer until it lets it go, as an owner who reads the picture for a while
  if (captureTestAnswer === "wait") {
    await new Promise((resolve) => (captureRelease = resolve));
    captureRelease = null;
    return captureTestAnswer === true;
  }
  if (captureTestAnswer !== null) return captureTestAnswer;
  const { preview, close } = await openPreview(png);
  try {
    const r = await dialog.showMessageBox(preview, {
      type: "question",
      message: t("capture.confirm.message"),
      detail: t("capture.confirm.detail"),
      buttons: [t("capture.confirm.yes"), t("capture.confirm.no")],
      defaultId: 1,
      cancelId: 1,
      noLink: true,
    });
    return r.response === 0;
  } finally {
    close();
  }
}

const captureScreen = createCapture({
  grab: async () => {
    const got = await grabScreen();
    if (got === "denied") {
      await dialog.showMessageBox({ type: "info", message: t("capture.denied.message"), detail: t("capture.denied.detail") });
      return null;
    }
    return got;
  },
  confirm: confirmPicture,
  // only to the service that was running when the picture was taken, and only if it still runs
  upload: (name, png) =>
    !current || state.stopped || current !== captureTarget
      ? Promise.resolve({ status: 0 })
      : adminClient(current.admin).postBytes(`/admin/attachments?name=${encodeURIComponent(name)}`, png),
  open: (hash) => showWindow(hash),
});
let lastCapture = null;
let captureTarget = null;

async function runCapture() {
  // a stopped service may have been one whose program changed: nothing goes to the address it had
  if (!current || state.stopped) return;
  captureTarget = current;
  let r;
  try {
    r = await captureScreen();
  } catch (e) {
    r = { outcome: "refused", status: 0 };
  }
  lastCapture = r;
  const why = { too_large: "capture.failed.large", refused: "capture.failed.refused" }[r.outcome];
  if (why && captureTestAnswer === null) await dialog.showMessageBox({ type: "warning", message: t("capture.failed.message"), detail: t(why) });
}

/** The keys that take a picture of the screen from anywhere, the same as the menu-bar entry: the picture is shown and
 *  nothing is attached without a yes. Taken only if no other app holds them, held only while the service runs, and with
 *  Shift as well, since apps use ⌥⌘K for themselves (Firefox's console). */
const CAPTURE_SHORTCUT = "CommandOrControl+Alt+Shift+K";

function takeShortcut() {
  let ok = false;
  try {
    ok = globalShortcut.register(CAPTURE_SHORTCUT, () => void runCapture());
  } catch {
    ok = false;
  }
  state.shortcut = ok ? CAPTURE_SHORTCUT : null;
}

/** The key is held while the service runs and the owner wants it from any app; it is let go when the service stops, as
 *  the menu entry is hidden then, and when the owner turns it off so other apps keep it. */
function syncShortcut() {
  if (state.stopped || !state.keyAnywhere) {
    if (globalShortcut.isRegistered(CAPTURE_SHORTCUT)) globalShortcut.unregister(CAPTURE_SHORTCUT);
    state.shortcut = null;
  } else if (!globalShortcut.isRegistered(CAPTURE_SHORTCUT)) {
    takeShortcut();
  }
}

const trayActions = {
  capture: () => void runCapture(),
  // another profile is another data folder with its own keys: the app starts again in it
  profile: (name) => {
    if (profileName === null || name === profileName || !chooseProfile(profilesHome, name)) return;
    // a development run driven by a test starts the app again itself
    if (runsFromAsar || !process.env.KAKUREGA_NO_RELAUNCH) app.relaunch();
    app.quit();
  },
  open: () => showWindow(),
  inbox: () => showWindow("#/inbox"),
  setNotify: (on) => {
    state.notify = on;
    saveSettings();
    refreshTray();
  },
  setKeyAnywhere: (on) => {
    state.keyAnywhere = on;
    saveSettings();
    refreshTray();
  },
  quit: () => app.quit(),
};

function refreshTray() {
  if (!tray || tray.isDestroyed()) return;
  syncShortcut();
  if (profileName !== null) {
    state.profile = profileName;
    state.profiles = [DEFAULT_PROFILE, ...profileNames(profilesHome)];
  }
  const key = JSON.stringify([state.count, state.stopped, state.notify, state.keyAnywhere, language, state.profile, state.profiles, state.shortcut]);
  // rebuilt only when something on it changed, so a menu the owner has open is not replaced under the pointer
  if (key === trayKey) return;
  trayKey = key;
  trayMenu = Menu.buildFromTemplate(menuTemplate(state, t, trayActions));
  tray.setContextMenu(trayMenu);
  tray.setToolTip(trayToolTip(state, t));
  if (macOS) tray.setTitle(trayTitle(state.count), { fontType: "monospacedDigit" });
}

function createTray() {
  try {
    const icon = nativeImage.createEmpty();
    icon.addRepresentation({ scaleFactor: 1, width: 16, height: 16, dataURL: `data:image/png;base64,${trayIconPng(16).toString("base64")}` });
    icon.addRepresentation({ scaleFactor: 2, width: 32, height: 32, dataURL: `data:image/png;base64,${trayIconPng(32).toString("base64")}` });
    icon.setTemplateImage(true);
    tray = new Tray(icon);
    if (process.platform === "win32") tray.on("click", () => showWindow());
    takeShortcut();
    refreshTray();
  } catch (e) {
    console.error("kakurega: no menu-bar item:", String(e?.message ?? e));
    tray = null;
  }
}

const notifier = createNotifier({
  hasFocus: () => !!window && !window.isDestroyed() && window.isVisible() && window.isFocused(),
  enabled: () => state.notify && Notification.isSupported(),
  text: (key) => t(key),
  show: (title, body, target, kind) => {
    const n = new Notification({ title, body, id: `kk-${kind}` });
    n.on("click", () => showWindow(target));
    n.on("failed", (_e, err) => console.error("kakurega: a notification could not be shown:", String(err)));
    notices.push({ title, body, target, notification: n });
    if (notices.length > 20) notices.shift();
    n.show();
  },
});

let polling = false;
async function pollInbox() {
  if (polling || !current) return null;
  polling = true;
  try {
    const r = await readInbox(current.client);
    state.count = r.items.length;
    state.complete = r.complete;
    const shown = notifier.observe(r.items);
    refreshTray();
    return { count: state.count, complete: r.complete, shown };
  } catch {
    return null;
  } finally {
    polling = false;
  }
}

async function start() {
  language = pickLanguage(app.commandLine.hasSwitch("lang") ? [app.getLocale()] : app.getPreferredSystemLanguages());
  t = translator(language);
  const changed = changedPart();
  if (changed) {
    refuseChanged(changed);
    app.isQuitting = true;
    // nothing was started, so there is nothing to wind down: leaving at once avoids the framework's own shutdown
    // steps, which have been seen to crash here and leave a "quit unexpectedly" report after the real message
    app.exit(1);
    return;
  }
  state.notify = readSettings().notify;
  state.keyAnywhere = readSettings().keyAnywhere;
  createTray();
  Menu.setApplicationMenu(Menu.buildFromTemplate(appMenu()));
  if (kakuBin) launchCompanion();
  await connect();
  const { admin, code } = current;
  session.defaultSession.setPermissionRequestHandler((_wc, _perm, cb) => cb(allowPermission()));
  session.defaultSession.setPermissionCheckHandler(() => allowPermission());
  session.defaultSession.setDevicePermissionHandler(() => false);

  session.defaultSession.webRequest.onBeforeSendHeaders({ urls: ["http://127.0.0.1/*"] }, (details, callback) => {
    // between a service that stopped and the one that replaces it there is nobody to send to: the request is ended at
    // once (the page shows that the service is not answering) and no secret is put on anything
    if (!current) return callback({ cancel: true });
    callback({ requestHeaders: withShellSecret(details.url, details.requestHeaders, current.admin.origin, shellSecret) });
  });
  session.defaultSession.webRequest.onCompleted({ urls: ["http://127.0.0.1/*"] }, (details) => {
    try {
      const u = new URL(details.url);
      if (details.statusCode === 401 && current && u.origin === current.admin.origin && u.pathname.startsWith("/api/") && u.pathname !== "/api/pair") void pairAgain();
      // Sign out answers 200 and nothing after it answers 401 (the page only asks whether it is paired), so it is seen here
      if (details.method === "POST" && details.statusCode < 300 && current && u.origin === current.admin.origin && u.pathname === "/api/logout") void pairAgain();
    } catch {
      /* not an address of ours */
    }
  });
  window = new BrowserWindow(windowOptions());
  window.webContents.setWindowOpenHandler(({ url }) => {
    // a page of the app itself that asks for a new tab (the capsule check) opens here: there is one window, and a
    // loopback address is never handed to the browser
    if (isAppUrl(url, appOrigin())) void window.loadURL(url);
    else if (mayOpenExternally(url)) shell.openExternal(url);
    return { action: "deny" };
  });
  window.webContents.on("will-navigate", (event, url) => {
    if (isAppUrl(url, appOrigin())) return;
    event.preventDefault();
    if (mayOpenExternally(url)) shell.openExternal(url);
  });
  window.webContents.on("will-redirect", (event, url) => {
    if (!isAppUrl(url, appOrigin())) event.preventDefault();
  });
  window.webContents.on("will-frame-navigate", (event) => {
    if (!isAppUrl(event.url, appOrigin())) event.preventDefault();
  });
  // will-navigate and will-frame-navigate do not fire for every target (about:blank slips through), so any
  // top-level page that is not the app is replaced at once.
  window.webContents.on("did-navigate", (_e, url) => {
    if (!isAppUrl(url, appOrigin()) && appOrigin()) window.loadURL(`${appOrigin()}/`).catch(() => {});
  });
  window.webContents.on("will-attach-webview", (event) => event.preventDefault());
  window.once("ready-to-show", () => window.show());
  // On macOS closing the window leaves Kakurega running in the menu bar, as Mac apps do; Quit (in the menu bar item or
  // Cmd-Q) ends it. Elsewhere closing the window quits.
  window.on("close", (event) => {
    if (app.isQuitting || !macOS || !tray || tray.isDestroyed()) return;
    event.preventDefault();
    window.hide();
  });
  window.webContents.on("render-process-gone", (_e, details) => {
    if (!rendererCrashed(details) || app.isQuitting) return;
    const d = rendererPolicy.next();
    if (!d.restart) {
      dialog.showErrorBox(t("error.window.title"), t("error.window.body"));
      return;
    }
    window.webContents.reload();
    tell(t("notice.reloaded.title"), t("notice.reloaded.body"));
  });
  let asking = false;
  window.on("unresponsive", async () => {
    if (asking || app.isQuitting) return;
    asking = true;
    const r = await dialog.showMessageBox(window, { type: "warning", title: t("unresponsive.title"), message: t("unresponsive.message"), buttons: [t("unresponsive.wait"), t("unresponsive.reload")], defaultId: 0, cancelId: 0 });
    asking = false;
    if (r.response === 1) window.webContents.reload();
  });
  await window.loadURL(`${admin.origin}/#pair=${code}`);

  const signer = (id) => {
    const f = enclaveSigner() ?? softwareSigner();
    return f ? f(id) : Promise.reject(new Error("no approver key is enrolled on this computer"));
  };
  Object.defineProperty(signer, "ownsDialog", { get: () => enclaveSigner() !== null });
  const pollOnce = createApprover({ client: { get: (p) => current.client.get(p), post: (p, b) => current.client.post(p, b) }, confirm, authenticate, sign: signer, seen: askedApprovals, t });
  setInterval(
    () =>
      pollOnce()
        .then((done) => {
          const waiting = done.filter((d) => d.outcome === "needs_enrolled_key");
          if (waiting.length > 0) {
            // with a key made, the same requests are asked again at the next look; without one the owner is told how
            for (const w of waiting) waitingForKey.add(w.id);
            void offerEnrol().then((made) => {
              if (!made) tell(t("notice.needsKey.title"), t("notice.needsKey.body"));
            });
          }
          if (done.some((d) => d.outcome === "authentication_failed")) tell(t("notice.touchFailed.title"), t("notice.touchFailed.body"));
        })
        .catch(() => {}),
    1000,
  );
  await pollInbox();
  setInterval(() => void pollInbox(), pollMs);
}

// A handle on the shell's own state for the end-to-end tests and for diagnosis. It lives in the main process only: the page
// has no bridge to it, and it holds nothing a notification would not show.
// It exists in a development run only: a packaged app has no such handle.
if (!runsFromAsar) {
  globalThis.kakuregaShell = Object.freeze({
    tray: () => tray,
    menu: () => trayMenu,
    window: () => window,
    state: () => ({ ...state }),
    language: () => language,
    notices: () => notices.map(({ title, body, target }) => ({ title, body, target })),
    clickNotice: (i) => notices[i]?.notification.emit("click"),
    pollInbox,
    // the picture and the owner's answer a test supplies in place of the screen and the dialog
    setCapture: (png, answer) => {
      captureTestPng = png;
      captureTestAnswer = answer;
    },
    capture: async () => {
      await runCapture();
      return lastCapture;
    },
    lastCapture: () => lastCapture,
    // what the menu and the key do when the service has stopped
    markStopped: (on) => {
      state.stopped = on;
      lastCapture = null;
      refreshTray();
    },
    // the service this app started stops, as it would by itself; the app reconnects as it does after any exit
    killService: () => child?.kill("SIGKILL"),
    connected: () => !!current && !state.stopped,
    // a capture whose answer comes after the service stopped: the picture is not sent
    captureAcrossStop: async () => {
      const before = captureTestAnswer;
      const oldPort = Number(new URL(current.admin.base).port);
      captureTestAnswer = "wait";
      const pending = runCapture();
      await sleep(50);
      child?.kill("SIGKILL");
      while (!state.stopped) await sleep(20);
      // something else now answers on the old address: it must hear nothing
      const { createServer } = await import("node:http");
      let heard = 0;
      const stranger = createServer((req, res) => {
        heard += 1;
        res.writeHead(201, { "content-type": "application/json" });
        res.end(JSON.stringify({ id: "att-000000000000" }));
      });
      await new Promise((resolve) => stranger.listen(oldPort, "127.0.0.1", resolve).on("error", resolve));
      captureTestAnswer = before;
      captureRelease?.();
      await pending;
      await new Promise((resolve) => stranger.close(resolve));
      return { ...lastCapture, heard };
    },
    shortcut: () => ({ keys: CAPTURE_SHORTCUT, registered: globalShortcut.isRegistered(CAPTURE_SHORTCUT), shown: state.shortcut }),
    // opens the preview of this picture with no dialog, says whether the picture is really drawn in it, and closes it
    previewShows: async (png) => {
      const before = readdirSync(app.getPath("temp")).filter((n) => n.startsWith("kakurega-preview-")).length;
      const { preview, close } = await openPreview(png);
      await sleep(400);
      const shot = await preview.webContents.capturePage();
      const bitmap = shot.toBitmap();
      // the window's own background is one dark colour; a drawn picture has many
      const seen = new Set();
      for (let i = 0; i + 3 < bitmap.length && seen.size < 40; i += 4 * 97) seen.add(`${bitmap[i]},${bitmap[i + 1]},${bitmap[i + 2]}`);
      close();
      const after = readdirSync(app.getPath("temp")).filter((n) => n.startsWith("kakurega-preview-")).length;
      return { colours: seen.size, bytes: png.length, left_behind: after - before };
    },
  });
}

// One window, one companion: a second launch only brings the first one forward.
if (!app.requestSingleInstanceLock()) {
  app.isQuitting = true;
  app.quit();
} else {
  app.on("second-instance", () => showWindow());
}

/** A preview folder left by a run that was killed while its dialog was open still holds a picture of the screen: at every
 *  start, any such folder is removed before anything else happens. */
function clearOldPreviews() {
  try {
    const temp = app.getPath("temp");
    for (const name of readdirSync(temp)) {
      if (name.startsWith("kakurega-preview-")) rmSync(join(temp, name), { recursive: true, force: true });
    }
  } catch {
    // the temp folder could not be listed: nothing of ours can be removed from it
  }
}

app.whenReady().then(() => { clearOldPreviews(); return app.isQuitting ? undefined : start(); }).catch((e) => {
  dialog.showErrorBox(t("error.start.title"), String(e.message ?? e));
  app.quit();
});
// the Dock icon of a running app whose window was closed brings the window back
app.on("activate", () => showWindow());

// A capsule (.kkg) or a share (.kks) opened from Finder, or dropped on the Dock icon, is checked here with the bundled kaku,
// offline, and the result is shown in a window of its own. Files opened before the app is ready wait until it is.
const opened = [];
let readyForFiles = false;
async function checkOpened(path) {
  if (!/\.(kkg|kks)$/i.test(path)) return;
  if (kakuFromBundle && checkPart(manifest, "kaku", kakuBin)) return;
  const result = await new Promise((resolve) => {
    if (!kakuBin) return resolve(null);
    execFile(kakuBin, ["verify", path], { timeout: 60000, maxBuffer: 1 << 20 }, (err, out) => resolve({ valid: !err && String(out).startsWith("VALID"), text: String(out) }));
  });
  if (!result) return;
  const lines = result.text.split("\n").filter((l) => !/^  \S+ answer [0-9a-f]{32}:/.test(l));
  const options = {
    type: result.valid ? "info" : "warning",
    message: t(result.valid ? "open.valid" : "open.invalid", { name: path.split("/").pop() }),
    detail: lines.slice(0, 60).join("\n"),
    buttons: [t("open.close")],
    noLink: true,
  };
  if (window && !window.isDestroyed()) {
    showWindow();
    await dialog.showMessageBox(window, options);
  } else {
    await dialog.showMessageBox(options);
  }
}
app.on("open-file", (event, path) => {
  event.preventDefault();
  if (readyForFiles) void checkOpened(path);
  else opened.push(path);
});
app.whenReady().then(() => {
  readyForFiles = true;
  for (const p of opened.splice(0)) void checkOpened(p);
});
app.on("will-quit", () => {
  globalShortcut.unregisterAll();
});

app.on("before-quit", () => {
  app.isQuitting = true;
  const c = child;
  child = null;
  // The companion is told by the closing of its pipe, so it can stop what it started (a helper waiting on a macOS
  // permission window would otherwise outlive the app). One that does not leave within three seconds is stopped anyway.
  if (c) {
    try {
      c.stdin.end();
      if (Number.isInteger(c.pid)) spawn("/bin/sh", ["-c", `sleep 3; kill ${c.pid} 2>/dev/null`], { detached: true, stdio: "ignore" }).unref();
    } catch {
      c.kill();
    }
  }
  // the companion this shell started is going away with it, so its admin file (port and token) goes too; a file written by
  // some other companion (a different port) is left alone
  try {
    if (c && current && JSON.parse(readFileSync(join(dataDir, "admin.json"), "utf8")).port === Number(new URL(current.admin.origin).port)) rmSync(join(dataDir, "admin.json"));
  } catch {}
});
app.on("window-all-closed", () => app.quit());
