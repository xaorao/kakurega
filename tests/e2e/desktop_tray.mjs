import "./quiet_crash.mjs";
import { _electron as electron } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import http from "node:http";

// The menu-bar item and the system notifications of the desktop app, driven in the real app with the real companion and a
// scripted model. The operating system's notification centre is not used: the shell's Notification.show is replaced in the
// main process for the run, and what it would have shown is read back. The inbox is read on demand (a long timer is set) so
// every check knows exactly which update it is looking at.
const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = process.env.KK_WORK ?? join(repo, "target/desktop-tray");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const project = join(root, "payments-service");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
const electronBin = process.env.ELECTRON_BIN ?? join(repo, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron");
const appDir = process.env.DESKTOP_APP_DIR ?? join(repo, "apps/desktop");
const kaku = process.env.KAKU_BIN ?? join(repo, "target/debug/kaku");
const { STRINGS } = await import(join(appDir, "src/strings.mjs"));
const webHi = readFileSync(join(repo, "apps/web/src/locales/hi.ts"), "utf8");

const results = [];
const check = (name, ok, detail = "") => {
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  " + detail : ""}`);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const admin = (data, method, path, body) =>
  new Promise((res) => {
    const a = JSON.parse(readFileSync(join(data, "admin.json"), "utf8"));
    const r = http.request({ host: "127.0.0.1", port: a.port, method, path, headers: { authorization: `Bearer ${a.token}`, ...(body ? { "content-type": "application/json" } : {}) } }, (m) => {
      let b = "";
      m.on("data", (c) => (b += c));
      m.on("end", () => {
        let json = null;
        try { json = JSON.parse(b); } catch {}
        res({ status: m.statusCode, json });
      });
    });
    r.on("error", () => res({ status: 0, json: null }));
    if (body) r.write(JSON.stringify(body));
    r.end();
  });
const companion = (data) => spawnSync("pgrep", ["-f", `serve --data-dir ${data}`], { encoding: "utf8" }).stdout.trim();

async function review(data, recipient) {
  const r = await admin(data, "POST", "/admin/jobs", { repo: project, recipient });
  const id = r.json?.id ?? r.json?.job;
  for (let i = 0; i < 360 && id; i++) {
    const j = await admin(data, "GET", `/admin/jobs/${id}`);
    if (j.json?.state === "ready") return id;
    if (j.json?.state === "failed") throw new Error(`review ${id} failed: ${j.json.note}`);
    await sleep(500);
  }
  throw new Error(`review did not get ready: ${JSON.stringify(r)}`);
}

async function launch(label, extraArgs = []) {
  const data = join(work, `data-${label}`);
  mkdirSync(data, { recursive: true });
  const app = await electron.launch({
    executablePath: electronBin,
    args: [...(process.platform === "darwin" ? ["--use-mock-keychain"] : ["--password-store=basic"]), ...extraArgs, appDir],
    env: { ...process.env, KAKU_BIN: kaku, KAKUREGA_DATA_DIR: data, KAKUREGA_INBOX_POLL_MS: "600000", KAKU_ARGS: `--root ${root} --scripted ${join(here, "script.json")} --ui-dir ${join(repo, "apps/web/dist")}` },
  });
  const page = await app.firstWindow();
  await page.waitForSelector("nav", { timeout: 40000 });
  // the operating system's notification centre is left alone: show() records instead
  await app.evaluate(({ Notification }) => {
    globalThis.__shownByOs = [];
    Notification.prototype.show = function () {
      globalThis.__shownByOs.push({ title: this.title, body: this.body });
    };
  });
  return { app, page, data };
}

const shell = (app, fn, arg) => app.evaluate(fn, arg);
const menuLabels = (app) => shell(app, () => globalThis.kakuregaShell.menu().items.filter((i) => i.type !== "separator").map((i) => ({ id: i.id, label: i.label, enabled: i.enabled, checked: i.checked })));
const clickMenu = (app, id) => shell(app, (_e, id) => globalThis.kakuregaShell.menu().getMenuItemById(id).click(), id);
const setFocus = (app, focused) => shell(app, (_e, f) => { const w = globalThis.kakuregaShell.window(); w.isFocused = () => f; }, focused);
const poll = (app) => shell(app, () => globalThis.kakuregaShell.pollInbox());

let app;
let data;
const secretRecipients = ["Quokka-Recipient-9902", "Ibex-Recipient-1203", "Lynx-Recipient-3304", "Oryx-Recipient-4405", "Tapir-Recipient-5506"];
try {
  ({ app, data } = await launch("en"));
  let page = await app.firstWindow();
  await page.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 40000 });
  const tray = await shell(app, () => { const t = globalThis.kakuregaShell.tray(); return t && { destroyed: t.isDestroyed(), title: process.platform === "darwin" ? t.getTitle() : "" }; });
  check("the app shows a menu-bar item as soon as it runs, with no number while nothing waits", !!tray && !tray.destroyed && tray.title === "", JSON.stringify(tray));
  const labels = await menuLabels(app);
  check("its menu says Kakurega is running and what waits, and offers Open, Waiting for you, notifications and Quit", JSON.stringify(labels.map((l) => l.label)) === JSON.stringify(["Kakurega is running", "Nothing is waiting for you", "Open Kakurega", "Waiting for you", "Take a picture of the screen for the chat…", "Tell me when something needs me", "Quit Kakurega"]) && labels[0].enabled === false && labels[1].enabled === false, JSON.stringify(labels.map((l) => l.label)));
  const adminInbox = await admin(data, "GET", "/admin/inbox");
  const st0 = await shell(app, () => globalThis.kakuregaShell.state());
  check(`the shell reads the inbox through its admin token (${adminInbox.status === 200 ? "the service's /admin/inbox" : "this service has no /admin/inbox, so the reviews' part from /admin/jobs"})`, st0.complete === (adminInbox.status === 200), `GET /admin/inbox -> ${adminInbox.status}`);

  // the owner is looking at the window: the count changes, nothing pops up
  await setFocus(app, true);
  await shell(app, () => globalThis.kakuregaShell.window().show());
  const first = await review(data, "Zebra-Recipient-Alpha-4471");
  const p1 = await poll(app);
  const title1 = await shell(app, () => (process.platform === "darwin" ? globalThis.kakuregaShell.tray().getTitle() : "1"));
  const l1 = await menuLabels(app);
  check("a finished review raises the count next to the icon and in the menu", p1?.count >= 1 && p1.complete === true && (process.platform !== "darwin" || title1 === String(p1.count)) && l1.find((l) => l.id === "inbox").label === `Waiting for you (${p1.count})` && l1.find((l) => l.id === "status").label.startsWith(`${p1.count} `), JSON.stringify([p1, title1]));
  check("while the window is in front no notification is shown", p1?.shown === 0 && (await shell(app, () => globalThis.__shownByOs.length)) === 0);

  // the owner is elsewhere: one generic notification, once
  await setFocus(app, false);
  const second = await review(data, secretRecipients[0]);
  const p2 = await poll(app);
  const shown2 = await shell(app, () => globalThis.__shownByOs);
  check("with the window not in front, a new review ready shows one notification", p2?.shown === 1 && shown2.length === 1, JSON.stringify(shown2));
  const words = shown2.map((n) => `${n.title} ${n.body}`).join(" | ");
  check("hostile: the notification says only that a review is ready: no recipient, no project, no review id, no file", shown2[0]?.title === "Kakurega" && shown2[0]?.body === "A review is ready." && !secretRecipients.some((s) => words.includes(s)) && !/payments|job-\d|\.env|README/.test(words), words);
  await poll(app);
  await poll(app);
  check("the same item is never announced twice", (await shell(app, () => globalThis.__shownByOs.length)) === 1);
  check("the review that finished while the owner was looking is not announced afterwards", !(await shell(app, () => globalThis.kakuregaShell.notices())).some((n) => n.target === `#/job/${first}`));

  // four new ones in one update: three are shown
  for (const r of secretRecipients.slice(1)) await review(data, r);
  const p3 = await poll(app);
  const p3b = await poll(app);
  check("at most three notifications at a time, and the rest are not saved up for later", p3?.shown === 3 && p3b?.shown === 0 && (await shell(app, () => globalThis.__shownByOs.length)) === 4, JSON.stringify([p3, p3b]));
  const allWords = (await shell(app, () => globalThis.__shownByOs)).map((n) => `${n.title} ${n.body}`).join(" ");
  check("none of them names a recipient", !secretRecipients.some((s) => allWords.includes(s)));

  // a click on the notification opens the window on that review
  await shell(app, () => globalThis.kakuregaShell.window().hide());
  await shell(app, () => globalThis.kakuregaShell.clickNotice(0));
  await page.waitForURL((u) => u.hash === `#/job/${second}`, { timeout: 10000 }).catch(() => {});
  const visible = await shell(app, () => globalThis.kakuregaShell.window().isVisible());
  await page.getByRole("heading", { name: "The exact release" }).waitFor({ timeout: 15000 }).catch(() => {});
  check("clicking a notification brings the window forward on that review's screen", visible && new URL(page.url()).hash === `#/job/${second}` && (await page.getByRole("heading", { name: "The exact release" }).count()) === 1, page.url());
  // the menu's "Waiting for you" opens the inbox
  await clickMenu(app, "inbox");
  await page.waitForURL((u) => u.hash === "#/inbox", { timeout: 10000 }).catch(() => {});
  await page.getByRole("heading", { name: "Waiting for you", level: 1 }).waitFor({ timeout: 15000 }).catch(() => {});
  check("Waiting for you in the menu opens the inbox screen", new URL(page.url()).hash === "#/inbox" && (await page.getByRole("heading", { name: "Waiting for you", level: 1 }).count()) === 1, page.url());

  // notifications can be turned off from the menu, and that is remembered
  await clickMenu(app, "notify");
  const off = await shell(app, () => globalThis.kakuregaShell.state().notify);
  const saved = JSON.parse(readFileSync(join(data, "electron/shell.json"), "utf8"));
  await review(data, "Wombat-Recipient-6607");
  const p4 = await poll(app);
  check("turning notifications off in the menu stops them, and the choice is saved", off === false && saved.notify === false && p4?.shown === 0 && (await menuLabels(app)).find((l) => l.id === "notify").checked === false, JSON.stringify([off, saved, p4]));
  await clickMenu(app, "notify");
  check("and on again", (await shell(app, () => globalThis.kakuregaShell.state().notify)) === true);

  // A picture of the screen for the chat. The screen itself and the owner's answer are supplied by the test: a picture of
  // the app's own window stands in for the screen (the operating system's permission is not something a test can grant).
  await shell(app, () => globalThis.kakuregaShell.window().show());
  await page.goto(new URL("#/privacy", page.url()).href);
  await page.getByRole("heading", { level: 1 }).first().waitFor();
  await shell(app, async () => {
    const png = (await globalThis.kakuregaShell.window().webContents.capturePage()).toPNG();
    globalThis.kakuregaShell.setCapture(png, false);
  });
  const discarded = await shell(app, () => globalThis.kakuregaShell.capture());
  check("a picture the owner discards is not attached and the screen does not change", discarded?.outcome === "discarded" && new URL(page.url()).hash === "#/privacy", JSON.stringify(discarded));
  await shell(app, async () => {
    const png = (await globalThis.kakuregaShell.window().webContents.capturePage()).toPNG();
    globalThis.kakuregaShell.setCapture(png, true);
  });
  await clickMenu(app, "capture");
  await page.waitForFunction(() => location.hash === "#/chat", null, { timeout: 30000 });
  const chip = page.locator("text=/screen-\\d{8}-\\d{6}\\.png/").first();
  await chip.waitFor({ timeout: 15000 });
  check("a picture the owner accepts from the menu opens the chat with the picture attached under a dated name", await chip.isVisible(), page.url());
  const done = await shell(app, () => globalThis.kakuregaShell.lastCapture());
  check("the shell reports it as attached, by an attachment's id", done?.outcome === "attached" && /^att-[0-9a-f]{12}$/.test(done?.id ?? ""), JSON.stringify(done));
  await page.screenshot({ path: join(work, "capture-chat.png") });
  const noToken = await fetch(`http://127.0.0.1:${JSON.parse(readFileSync(join(data, "admin.json"), "utf8")).port}/admin/attachments?name=x.png`, { method: "POST", body: Buffer.from([1, 2, 3]) });
  check("the picture's door needs the shell's own token", noToken.status === 401, String(noToken.status));
  const perms = await page.evaluate(async () => {
    try {
      await navigator.mediaDevices.getDisplayMedia({ video: true });
      return "granted";
    } catch (e) {
      return String(e?.name ?? e);
    }
  });
  check("the page itself still cannot take a picture of the screen", perms !== "granted", perms);
  await shell(app, () => globalThis.kakuregaShell.setCapture(null, null));

  // closing the window keeps Kakurega in the menu bar (macOS)
  const companionBefore = companion(data);
  await shell(app, () => globalThis.kakuregaShell.window().close());
  await sleep(1500);
  const afterClose = await shell(app, () => { const w = globalThis.kakuregaShell.window(); return { destroyed: w.isDestroyed(), visible: !w.isDestroyed() && w.isVisible(), tray: !globalThis.kakuregaShell.tray().isDestroyed() }; });
  const stillServing = await admin(data, "GET", "/admin/overview");
  if (process.platform === "darwin") {
    check("closing the window leaves the app running in the menu bar, with its companion and its data", afterClose.destroyed === false && afterClose.visible === false && afterClose.tray && companion(data) === companionBefore && companionBefore !== "" && stillServing.status === 200, JSON.stringify(afterClose));
    await clickMenu(app, "open");
    await sleep(800);
    check("Open Kakurega brings the same window back", await shell(app, () => globalThis.kakuregaShell.window().isVisible()));
    // a second launch while the window is closed brings it back too, and starts nothing
    await shell(app, () => globalThis.kakuregaShell.window().close());
    await sleep(800);
    const second2 = await new Promise((done) => {
      const c = spawn(electronBin, ["--use-mock-keychain", appDir], { env: { ...process.env, KAKU_BIN: kaku, KAKUREGA_DATA_DIR: data }, stdio: "ignore" });
      const timer = setTimeout(() => { c.kill("SIGKILL"); done({ exited: false }); }, 15000);
      c.on("exit", (code) => { clearTimeout(timer); done({ exited: true, code }); });
    });
    await sleep(800);
    check("a second launch while the window is closed exits at once and brings the window back", second2.exited && (await shell(app, () => globalThis.kakuregaShell.window().isVisible())) && companion(data) === companionBefore, JSON.stringify(second2));
  }

  // the window's process crashes: the menu-bar item is not part of it and keeps working
  await shell(app, () => globalThis.kakuregaShell.window().webContents.forcefullyCrashRenderer());
  await sleep(3000);
  await clickMenu(app, "inbox");
  const afterCrash = await shell(app, async () => {
    const w = globalThis.kakuregaShell.window();
    const shown = await w.webContents.executeJavaScript(`new Promise(async (res) => { for (let i = 0; i < 60; i++) { if (location.hash === "#/inbox" && document.querySelector("#h-inbox")) break; await new Promise(r => setTimeout(r, 250)); } res({ hash: location.hash, inbox: !!document.querySelector("#h-inbox") }); })`);
    return { tray: !globalThis.kakuregaShell.tray().isDestroyed(), polled: await globalThis.kakuregaShell.pollInbox(), ...shown };
  });
  check("after the window crashes the menu-bar item is still there, still reads the inbox, and still opens the inbox screen", afterCrash.tray && afterCrash.polled?.count >= 1 && afterCrash.hash === "#/inbox" && afterCrash.inbox, JSON.stringify(afterCrash));

  // Quit in the menu ends the app and its companion
  const pid = app.process().pid;
  await clickMenu(app, "quit").catch(() => {});
  let gone = false;
  for (let i = 0; i < 40 && !gone; i++) {
    await sleep(250);
    try { process.kill(pid, 0); } catch { gone = true; }
  }
  for (let i = 0; i < 20 && companion(data) !== ""; i++) await sleep(250);
  check("Quit Kakurega ends the app and its companion", gone && companion(data) === "" && !existsSync(join(data, "admin.json")), `app gone=${gone} companion=${companion(data)}`);
  app = null;

  // the shell follows the system language: in Hindi its menu and its notifications are in Hindi
  ({ app, data } = await launch("hi", ["--lang=hi"]));
  page = await app.firstWindow();
  await page.waitForLoadState("domcontentloaded");
  const lang = await shell(app, () => globalThis.kakuregaShell.language());
  const hiLabels = (await menuLabels(app)).map((l) => l.label);
  check("with the system in Hindi the menu-bar item's menu is in Hindi", lang === "hi" && JSON.stringify(hiLabels) === JSON.stringify([STRINGS.hi["tray.running"], STRINGS.hi["tray.nothing"], STRINGS.hi["tray.open"], STRINGS.hi["tray.inbox"], STRINGS.hi["tray.capture"], STRINGS.hi["tray.notify"], STRINGS.hi["tray.quit"]]), JSON.stringify([lang, hiLabels]));
  await setFocus(app, false);
  await poll(app);
  await review(data, "Hindi-Recipient-7708");
  await poll(app);
  const hiShown = await shell(app, () => globalThis.__shownByOs);
  const webWords = /"notify\.ready_to_review": "([^"]*)"/.exec(webHi)?.[1];
  check("and a notification uses the web app's Hindi words", hiShown.length === 1 && hiShown[0].body === webWords && !hiShown[0].body.includes("Hindi-Recipient"), JSON.stringify(hiShown));
} catch (e) {
  check("the run finished without an error", false, String(e?.stack ?? e).slice(0, 400));
} finally {
  await app?.close().catch(() => {});
  for (const d of ["en", "hi"]) spawnSync("pkill", ["-9", "-f", `serve --data-dir ${join(work, `data-${d}`)}`]);
}
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} menu-bar and notification checks passed` : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
