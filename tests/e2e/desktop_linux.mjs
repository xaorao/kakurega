import { _electron as electron } from "playwright-core";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, renameSync, rmSync, readFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Functional desktop flow on a platform without Touch ID or a Secure Enclave: the shell must still require the owner's
// explicit yes in its own dialog and a signed approval from an enrolled key, and must refuse everything else.
const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = process.env.KK_WORK ?? join(repo, "target/desktop-linux-tmp");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
const project = join(root, "payments-service");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), project, { recursive: true });
renameSync(join(project, "_env"), join(project, ".env"));
const data = join(work, "data");
mkdirSync(data, { recursive: true });
const kaku = process.env.KAKU_BIN ?? join(repo, "target/debug/kaku");
const results = [];
const check = (n, ok, d = "") => (results.push(ok), console.log(`${ok ? "PASS" : "FAIL"}  ${n}${d ? "  " + d : ""}`));

const app = await electron.launch({
  executablePath: process.env.ELECTRON_BIN,
  args: [...(process.platform === "darwin" ? ["--use-mock-keychain"] : ["--password-store=basic"]), process.env.DESKTOP_APP_DIR ?? join(repo, "apps/desktop")],
  env: { ...process.env, KAKU_BIN: kaku, KAKUREGA_DATA_DIR: data, KAKU_ARGS: `--root ${root} --scripted ${join(here, "script.json")} --ui-dir ${join(repo, "apps/web/dist")}` },
});
const problems = [];
try {
  const page = await app.firstWindow();
  page.on("console", (m) => m.type() === "error" && problems.push(m.text()));
  page.on("pageerror", (e) => problems.push(e.message));
  await page.getByRole("heading", { name: "Review a project" }).waitFor({ timeout: 40000 });
  const origin = new URL(page.url()).origin;
  check("the window opens the workspace already paired", true);
  await app.evaluate(({ dialog }) => {
    globalThis.__shown = [];
    globalThis.__answer = 1;
    dialog.showMessageBox = async (opts) => { globalThis.__shown.push({ detail: opts.detail, buttons: opts.buttons }); return { response: globalThis.__answer }; };
  });
  const start = async (who) => {
    await page.goto(origin + "/");
    await page.getByRole("button", { name: "payments-service" }).click();
    await page.getByLabel("Recipient name").fill(who);
    await page.getByRole("button", { name: "Start review" }).click();
    await page.getByRole("heading", { name: "The exact release" }).waitFor({ timeout: 180000 });
    await page.getByRole("button", { name: "Request approval" }).click();
  };
  // 1. no Touch ID and no key: approving is impossible, and the shell says so by refusing
  await app.evaluate(() => { globalThis.__answer = 1; });
  await start("no-key-audit");
  await page.getByText(/refused|denied|not approved/i).first().waitFor({ timeout: 20000 }).catch(() => {});
  check("with no Touch ID and no approver key nothing can be approved: the Deliver button never appears", (await page.getByRole("button", { name: "Sign and deliver the capsule" }).count()) === 0);
  // 2. enrol a software key; the shell shows its dialog, then signs through kaku
  const enr = spawnSync(kaku, ["approver", "enroll", "--data-dir", data, "--label", "linux e2e"], { encoding: "utf8" });
  check("a software approver key is enrolled", enr.status === 0, (enr.stdout + enr.stderr).trim().slice(0, 80));
  await app.evaluate(() => { globalThis.__shown.length = 0; globalThis.__answer = 0; });
  await start("refused-audit");
  await page.waitForTimeout(4000);
  const shownRefuse = await app.evaluate(() => globalThis.__shown);
  check("when the owner refuses in the shell's dialog nothing is signed or approved", shownRefuse.length >= 1 && (await page.getByRole("button", { name: "Sign and deliver the capsule" }).count()) === 0);
  await app.evaluate(() => { globalThis.__shown.length = 0; globalThis.__answer = 1; });
  await start("linux-audit");
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).waitFor({ timeout: 30000 });
  const shown = await app.evaluate(() => globalThis.__shown);
  check("the dialog listed the recipient, the files and what is withheld, with Refuse as the first button", shown.length >= 1 && /linux-audit/.test(shown[0].detail) && /finding-1\.txt/.test(shown[0].detail) && /Withheld/.test(shown[0].detail) && shown[0].buttons[0] === "Refuse", shown[0]?.detail?.slice(0, 60));
  await page.getByRole("button", { name: "Sign and deliver the capsule" }).click();
  await page.getByText("Delivered.").waitFor({ timeout: 30000 });
  check("after the owner's yes and a signature from the enrolled key the release is delivered", true);
  check("no console or page errors", problems.length === 0, problems.join(" | ").slice(0, 200));
} finally {
  await app.close();
}
const bad = results.filter((r) => !r).length;
console.log(bad === 0 ? `\nall ${results.length} desktop (no Touch ID) checks passed` : `\n${bad} FAILED`);
process.exit(bad ? 1 : 0);
