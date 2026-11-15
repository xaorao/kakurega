// A downloaded app only takes signed approvals and starts with no key. The app must offer to make one itself, and after
// that the waiting release must be asked for and approved; "Not now" must leave the release waiting and say what to do.
// Sign out pressed in the window must not leave a pairing screen nobody here can answer.
import "./quiet_crash.mjs";
import { _electron as electron } from "playwright-core";
import { spawnSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/desktop-enrol-tmp");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const project = join(root, "payments-service");
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
const data = join(work, "data");
mkdirSync(data, { recursive: true });

// a stand-in calendar reader that waits, as the real one does while macOS shows its permission window
const reader = join(work, "slow-calendar-reader");
writeFileSync(reader, "#!/bin/sh\nsleep 120\n");
chmodSync(reader, 0o755);
const readerRunning = () => spawnSync("pgrep", ["-f", reader], { encoding: "utf8" }).stdout.trim() !== "";

const results = [];
const check = (name, ok, detail = "") => {
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  " + detail : ""}`);
};

const app = await electron.launch({
  executablePath: join(repo, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"),
  args: ["--use-mock-keychain", join(repo, "apps/desktop")],
  env: {
    ...process.env,
    KAKU_BIN: join(repo, "target/debug/kaku"),
    KAKUREGA_DATA_DIR: data,
    KAKU_CALENDAR: reader,
    KAKU_CALENDAR_UNCONFINED: "1",
    // as a packaged app starts its service: signed approvals only
    KAKU_ARGS: `--require-approver --root ${root} --scripted ${join(here, "script.json")} --ui-dir ${join(repo, "apps/web/dist")}`,
  },
});
try {
  const page = await app.firstWindow();
  await page.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 30000 });
  // the native windows are answered here: the first offer is declined, every later one accepted
  await app.evaluate(({ dialog, systemPreferences, Notification }) => {
    globalThis.__shown = [];
    globalThis.__told = [];
    globalThis.__enrolAnswers = [0, 1];
    dialog.showMessageBox = async (...args) => {
      const o = args[args.length - 1];
      const enrol = /Set up approval/.test(o.message ?? "");
      globalThis.__shown.push({ kind: enrol ? "enrol" : "approve", message: o.message, detail: o.detail, buttons: o.buttons });
      return { response: enrol ? (globalThis.__enrolAnswers.shift() ?? 1) : 1 };
    };
    dialog.showErrorBox = (title, body) => globalThis.__shown.push({ kind: "error", message: title, detail: body });
    systemPreferences.canPromptTouchID = () => false;
    const show = Notification.prototype.show;
    Notification.prototype.show = function () { globalThis.__told.push(this.title + " | " + this.body); };
    void show;
  });
  const origin = new URL(page.url()).origin;
  const review = async (recipient) => {
    await page.goto(origin + "/");
    await page.getByRole("button", { name: "payments-service" }).click();
    await page.getByLabel("Recipient name").fill(recipient);
    await page.getByRole("button", { name: "Start review" }).click();
    await page.getByRole("heading", { name: "The exact release" }).waitFor({ timeout: 120000 });
    await page.getByRole("button", { name: "Request approval" }).click();
  };
  const shown = () => app.evaluate(() => globalThis.__shown);
  const until = async (f, ms = 20000) => { const end = Date.now() + ms; while (Date.now() < end) { const v = await f(); if (v) return v; await new Promise((r) => setTimeout(r, 300)); } return null; };

  await review("acme-audit");
  const first = await until(async () => (await shown()).find((x) => x.kind === "enrol"));
  check("with no key on this computer the app itself offers to set up approval", !!first, JSON.stringify((await shown()).map((x) => x.kind)));
  check("the offer says what is made and where, and that this Mac has no Touch ID", /signing key/.test(first?.detail ?? "") && /only your user account can read/.test(first?.detail ?? "") && /no Touch ID/.test(first?.detail ?? ""), first?.detail?.slice(0, 120));
  check("the offer can be declined: Not Now is the first button, Set Up the second", first?.buttons?.[0] === "Not Now" && first?.buttons?.[1] === "Set Up", JSON.stringify(first?.buttons));
  const told = await until(async () => (await app.evaluate(() => globalThis.__told)).find((x) => /not set up to approve yet/.test(x)));
  check("declined, the owner is told where to set it up, in words that need no terminal", !!told && /Kakurega menu/.test(told) && !/kaku approver/.test(told), told ?? "");
  check("declined, no key was made and nothing was approved", !existsSync(join(data, "approver.key")) && !(await shown()).some((x) => x.kind === "approve"));

  // the menu item makes the same offer; accepted this time
  const made = app.evaluate(({ Menu }) => { const item = Menu.getApplicationMenu().items.flatMap((m) => m.submenu?.items ?? []).find((i) => /Set Up Approval/.test(i.label)); if (!item) return false; item.click(); return true; });
  check("the Kakurega menu has Set Up Approval on This Mac", await made);
  await until(() => existsSync(join(data, "approver.key")));
  check("accepted, kaku made the key file in the data folder", existsSync(join(data, "approver.key")));
  const done = await until(async () => (await app.evaluate(() => globalThis.__told)).find((x) => /Approval is set up/.test(x)));
  check("and the owner is told it is set up", !!done, done ?? "");

  const waited = await until(async () => (await shown()).find((x) => x.kind === "approve" && /acme-audit/.test(x.detail ?? "")));
  check("the release that was waiting for a key is now put to the owner, with no second request", !!waited, JSON.stringify((await shown()).map((x) => x.kind)));
  check("its window says this Mac has no Touch ID, so the click is the whole confirmation... only when no key signs", !/no Touch ID/.test(waited?.detail ?? ""), waited?.detail?.slice(0, 60));

  // a release requested now is asked for in the native window and approved with the new key
  await review("globex-audit");
  const sheet = await until(async () => (await shown()).find((x) => x.kind === "approve" && /globex-audit/.test(x.detail ?? "")));
  check("with the key made, a requested release is asked for in the native window", !!sheet, JSON.stringify((await shown()).map((x) => x.kind)));
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).waitFor({ timeout: 20000 });
  check("and the signed approval is accepted by a service that takes signed approvals only", true);
  check("no error box was shown along the way", !(await shown()).some((x) => x.kind === "error"), JSON.stringify((await shown()).filter((x) => x.kind === "error")));

  // a capsule opened from Finder (or dropped on the Dock icon) is checked offline and the result shown in a window
  const sample = join(repo, "fixtures/near-live/sample.kkg");
  const changed = join(work, "changed.kkg");
  const bytes = readFileSync(sample);
  bytes[Math.floor(bytes.length / 2)] ^= 1;
  writeFileSync(changed, bytes);
  await app.evaluate(({ app }, p) => app.emit("open-file", { preventDefault() {} }, p), sample);
  const good = await until(async () => (await shown()).find((x) => /is valid/.test(x.message ?? "")));
  check("a capsule opened from Finder is checked offline and said to be valid", !!good && /sample\.kkg is valid/.test(good.message) && /VALID/.test(good.detail ?? ""), good?.message);
  await app.evaluate(({ app }, p) => app.emit("open-file", { preventDefault() {} }, p), changed);
  const bad = await until(async () => (await shown()).find((x) => /is not valid/.test(x.message ?? "")));
  check("a changed capsule opened the same way is said not to be valid, with the reason", !!bad && /INVALID/.test(bad.detail ?? ""), bad?.message);

  // Sign out in the window: the app pairs its own window again
  await page.getByRole("button", { name: "Sign out" }).click();
  const back = await until(async () => (await page.getByRole("heading", { name: "Review a project" }).count()) > 0 || ((await page.goto(origin + "/").catch(() => null)), false), 15000);
  const pairing = await page.getByText("Pair this browser").count();
  check("after Sign out the window comes back paired by itself, not to a pairing screen", !!back && pairing === 0, `pairing screen: ${pairing}`);

  // quitting while a calendar reader waits: the reader goes with the app
  const again = await app.firstWindow();
  await again.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 20000 });
  void again.evaluate(() => { const csrf = document.cookie; void csrf; return fetch("/api/session").then((r) => r.json()).then((s) => fetch("/api/attachments/calendar", { method: "POST", headers: { "content-type": "application/json", "x-kk-csrf": s.csrf }, body: JSON.stringify({ days: 7, reminders: true, ask: true }) })).catch(() => null); }).catch(() => {});
  const started = await until(() => readerRunning(), 10000);
  check("a calendar reader is waiting (the stand-in for the macOS permission window)", !!started);
  await app.evaluate(({ app }) => app.quit()).catch(() => {});
  const gone = await until(() => !readerRunning(), 8000);
  check("quitting the app stops the waiting reader; nothing of it is left behind", !!gone);
  const servedGone = await until(() => spawnSync("pgrep", ["-f", `serve --data-dir ${data}`], { encoding: "utf8" }).stdout.trim() === "", 6000);
  check("and the service is gone too", !!servedGone);
} finally {
  await app.close().catch(() => {});
  spawnSync("pkill", ["-f", reader]);
}
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} desktop set-up checks passed` : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
