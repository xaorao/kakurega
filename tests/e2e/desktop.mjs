import { _electron as electron } from "playwright-core";
import { cpSync, mkdirSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = join(repo, "target/desktop-tmp");
const shots = join(repo, "target/e2e-shots");
rmSync(work, { recursive: true, force: true });
mkdirSync(shots, { recursive: true });
const root = join(work, "root");
const project = join(root, "payments-service");
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
const data = join(work, "data");
mkdirSync(data, { recursive: true });

const results = [];
const check = (name, ok, detail = "") => {
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  " + detail : ""}`);
};

const app = await electron.launch({
  executablePath: join(repo, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"),
  args: [join(repo, "apps/desktop")],
  env: {
    ...process.env,
    KAKU_BIN: join(repo, "target/debug/kaku"),
    KAKUREGA_DATA_DIR: data,
    KAKU_ARGS: `--root ${root} --scripted ${join(here, "script.json")} --ui-dir ${join(repo, "apps/web/dist")}`,
  },
});
const problems = [];
try {
  const page = await app.firstWindow();
  page.on("console", (m) => m.type() === "error" && problems.push("console: " + m.text()));
  page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
  await page.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 30000 });
  check("the window opens the workspace already paired, no link typed", true);
  check("the pairing code is gone from the address", !page.url().includes("pair="), page.url());
  await page.screenshot({ path: join(shots, "desktop-01-workspace.png") });

  const exposure = await page.evaluate(() => ({ require: typeof require, process: typeof process, electron: typeof window.electron, ipc: typeof window.ipcRenderer }));
  check("the page has no Node, no require, no electron bridge", Object.values(exposure).every((v) => v === "undefined"), JSON.stringify(exposure));

  const origin = new URL(page.url()).origin;
  const popup = await page.evaluate(() => window.open("https://example.com/") === null);
  await new Promise((r) => setTimeout(r, 800));
  check("window.open to another site opens no app window", popup && app.windows().length === 1, `windows=${app.windows().length}`);
  await page.evaluate(() => { location.href = "https://example.com/"; });
  await new Promise((r) => setTimeout(r, 1500));
  check("navigating the app window to another site is refused", new URL(page.url()).origin === origin, page.url());
  check("no console or page errors", problems.length === 0, problems.join(" | "));
  const perm = await page.evaluate(() => new Promise((res) => navigator.geolocation.getCurrentPosition(() => res("granted"), (e) => res(e.code === 1 ? "denied" : "other:" + e.code), { timeout: 3000 })));
  check("location is refused by the permission policy", perm === "denied", perm);

  // Approval through the native layer. The two OS prompts are replaced in the main process only for this test;
  // each stub records what it was shown so we can check the dialog carried the exact release.
  await app.evaluate(({ dialog, systemPreferences }) => {
    globalThis.__shown = [];
    dialog.showMessageBox = async (opts) => { globalThis.__shown.push({ kind: "dialog", detail: opts.detail, buttons: opts.buttons }); return { response: 1 }; };
    systemPreferences.canPromptTouchID = () => true;
    systemPreferences.promptTouchID = async (reason) => { globalThis.__shown.push({ kind: "touchid", reason }); };
  });
  const door = await page.evaluate(async () => { const r1 = await fetch("/admin/approvals"); const r2 = await fetch("/admin/approvals/x/approve", { method: "POST", headers: { "content-type": "application/json" }, body: "{\"ack\":\"x\"}" }); return [r1.status, r2.status]; });
  check("a page script cannot list or answer approvals (no admin credential reachable)", door.every((c) => c === 401 || c === 403), JSON.stringify(door));
  problems.length = 0;
  await page.goto(origin + "/");
  await page.getByRole("button", { name: "payments-service" }).click();
  await page.getByLabel("Recipient name").fill("acme-audit");
  await page.getByRole("button", { name: "Start review" }).click();
  await page.getByRole("heading", { name: "The exact release" }).waitFor({ timeout: 120000 });
  await page.getByRole("button", { name: "Request approval" }).click();
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).waitFor({ timeout: 20000 });
  const shown = await app.evaluate(() => globalThis.__shown);
  const dlg = shown.find((x) => x.kind === "dialog");
  const tid = shown.find((x) => x.kind === "touchid");
  check("the native dialog listed the recipient, the exact files and what is withheld", !!dlg && /acme-audit/.test(dlg.detail) && /finding-1\.txt/.test(dlg.detail) && /Withheld/.test(dlg.detail), dlg?.detail?.slice(0, 80));
  check("the default button is Refuse and approval is a separate choice", dlg?.buttons?.[0] === "Refuse" && dlg?.buttons?.[1].includes("Touch ID"));
  check("the operating system proof was asked after the dialog and named the digest", !!tid && /Approve release [0-9a-f]{12} to acme-audit/.test(tid.reason), tid?.reason);
  await page.screenshot({ path: join(shots, "desktop-02-approved.png") });
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).click();
  await page.getByText("Delivered.").waitFor({ timeout: 20000 });
  check("after native approval the release is signed and delivered", true);
  check("no console or page errors after the full flow", problems.length === 0, problems.join(" | "));
} finally {
  await app.close();
}
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} desktop checks passed` : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
