import "./quiet_crash.mjs";
import { spawnSync } from "node:child_process";
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
  args: ["--use-mock-keychain", join(repo, "apps/desktop")],
  env: {
    ...process.env,
    KAKU_BIN: join(repo, "target/debug/kaku"),
    KAKU_ENCLAVE_BIN: join(repo, "target/debug/kaku-enclave-test"),
    KAKUREGA_DATA_DIR: data,
    // a model name as the first argument runs the same flows with that real local model (minutes, not seconds)
    KAKU_ARGS: `--root ${root} ${process.argv[2] ? `--model ${process.argv[2]}` : `--scripted ${join(here, "script.json")}`} --ui-dir ${join(repo, "apps/web/dist")}`,
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

  // another program on this computer (this test runner is one) is refused everywhere, with and without a guess at the secret
  const appOrigin = new URL(page.url()).origin;
  for (const [path, init] of [["/", {}], ["/api/health", {}], ["/api/session", {}], ["/api/pair", { method: "POST", headers: { Origin: appOrigin, "content-type": "application/json" }, body: JSON.stringify({ code: "x" }) }], ["/", { headers: { "x-kk-shell": "guess".repeat(10) } }]]) {
    const r = await fetch(appOrigin + path, init);
    const body = await r.text();
    check(`an outside program is refused at ${path}${init.headers?.["x-kk-shell"] ? " even with a guessed secret" : ""}`, r.status === 403 && body.includes("SHELL_ONLY"), `${r.status} ${body.slice(0, 80)}`);
  }

  await app.evaluate(({ shell }) => { shell.openExternal = async () => {}; });
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
    dialog.showMessageBox = async (...args) => { const opts = args[args.length - 1]; globalThis.__shown.push({ kind: "dialog", detail: opts.detail, buttons: opts.buttons }); return { response: 1 }; };
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
  await page.getByRole("heading", { name: "The exact release" }).waitFor({ timeout: process.argv[2] ? 900000 : 120000 });
  await page.getByRole("button", { name: "Request approval" }).click();
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).waitFor({ timeout: 20000 });
  const shown = await app.evaluate(() => globalThis.__shown);
  const dlg = shown.find((x) => x.kind === "dialog");
  const tid = shown.find((x) => x.kind === "touchid");
  check("the native dialog listed the recipient, the exact files and what is withheld", !!dlg && /acme-audit/.test(dlg.detail) && /finding-1\.txt/.test(dlg.detail) && /Withheld/.test(dlg.detail), dlg?.detail?.slice(0, 80));
  check("the native dialog says how long the release can still be approved, from the service's own clock", !!dlg && /This release can be approved for another \d+ h( \d+ min)?\./.test(dlg.detail) && !/expired|could not be read/.test(dlg.detail), dlg?.detail?.split("\n").slice(0, 4).join(" | "));
  check("the default button is Refuse and approval is a separate choice", dlg?.buttons?.[0] === "Refuse" && dlg?.buttons?.[1].includes("Touch ID"));
  check("the operating system proof was asked after the dialog and named the digest", !!tid && /Approve release [0-9a-f]{12} to acme-audit/.test(tid.reason), tid?.reason);
  await page.screenshot({ path: join(shots, "desktop-02-approved.png") });
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).click();
  await page.getByText("Delivered.").waitFor({ timeout: 20000 });
  check("after native approval the release is signed and delivered", true);
  // second round: a Secure Enclave approver key is enrolled, so the shell must sign inside the enclave
  const enr = spawnSync(join(repo, "target/debug/kaku"), ["approver", "enroll", "--enclave", "--data-dir", data, "--label", "e2e enclave"], { encoding: "utf8", env: { ...process.env, KAKU_ENCLAVE_BIN: join(repo, "target/debug/kaku-enclave-test") } });
  check("a Secure Enclave approver key is enrolled", enr.status === 0, (enr.stdout + enr.stderr).trim().slice(0, 100));
  await app.evaluate(() => { globalThis.__shown.length = 0; });
  await page.goto(origin + "/");
  await page.getByRole("button", { name: "payments-service" }).click();
  await page.getByLabel("Recipient name").fill("enclave-audit");
  await page.getByRole("button", { name: "Start review" }).click();
  await page.getByRole("heading", { name: "The exact release" }).waitFor({ timeout: process.argv[2] ? 900000 : 120000 });
  await page.getByRole("button", { name: "Request approval" }).click();
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).waitFor({ timeout: 30000 });
  const shown2 = await app.evaluate(() => globalThis.__shown);
  check("the shell asked nothing itself: the Secure Enclave helper owns the dialog and the signature, and the Touch ID stub was not used", shown2.length === 0, JSON.stringify(shown2.map((x) => x.kind)));
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).click();
  await page.getByText("Delivered.").waitFor({ timeout: 20000 });
  check("the enclave-signed approval delivered", true);
  await page.getByRole("link", { name: "Documents" }).click();
  await page.getByRole("heading", { name: "Ask your documents", exact: true }).waitFor({ timeout: 20000 });
  const first = page.getByRole("button", { name: /payments|contracts|docs/i }).first();
  await first.click();
  await page.getByLabel("Your question").fill("What does this folder describe?");
  await page.getByRole("button", { name: "Ask", exact: true }).click();
  await page.getByRole("heading", { name: /Answer|Unchecked/i }).first().waitFor({ timeout: process.argv[2] ? 300000 : 60000 });
  const card = await page.locator("#h-answer").locator("xpath=..").innerText().catch(() => "");
  check("document questions work in the desktop window and show a checked or clearly unchecked answer", /not proof|not a checked|unchecked/i.test(card) || card.length > 40, card.replace(/\n/g, " | ").slice(0, 160));
  await page.locator("#h-answer").scrollIntoViewIfNeeded().catch(() => {});
  await page.screenshot({ path: join(shots, "desktop-03-documents.png") });
  check("no console or page errors after the full flow", problems.length === 0, problems.join(" | "));
} finally {
  await app.close();
}
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} desktop checks passed` : `\n${bad} FAILED`);
process.exit(bad === 0 ? 0 : 1);
