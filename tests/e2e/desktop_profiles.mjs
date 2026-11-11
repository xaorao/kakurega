import "./quiet_crash.mjs";
import { _electron as electron } from "playwright-core";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Profiles in the desktop app: the menu lists them, choosing one saves the choice and ends the app, and the next start
// works in that profile's own folder with its own keys. Real app, real companion, scripted model.
const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const work = process.env.KK_WORK ?? join(repo, "target/desktop-profiles");
rmSync(work, { recursive: true, force: true });
const fakeHome = join(work, "home");
const home = join(fakeHome, ".kakurega");
const root = join(work, "root");
mkdirSync(root, { recursive: true });
mkdirSync(fakeHome, { recursive: true });
const electronBin = process.env.ELECTRON_BIN ?? join(repo, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron");
const kaku = process.env.KAKU_BIN ?? join(repo, "target/debug/kaku");
let failed = 0;
let passed = 0;
const check = (name, ok, detail = "") => {
  ok ? passed++ : failed++;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${!ok && detail ? `  ${detail}` : ""}`);
};
const env = { ...process.env, HOME: fakeHome, KAKUREGA_HOME: home, KAKU_BIN: kaku, KAKUREGA_NO_RELAUNCH: "1", KAKUREGA_INBOX_POLL_MS: "600000", KAKU_ARGS: `--root ${root} --scripted ${join(here, "script.json")} --ui-dir ${join(repo, "apps/web/dist")}` };
delete env.KAKUREGA_DATA_DIR;
delete env.KAKU_PROFILE;

async function launch() {
  const app = await electron.launch({ executablePath: electronBin, args: [...(process.platform === "darwin" ? ["--use-mock-keychain"] : ["--password-store=basic"]), join(repo, "apps/desktop")], env });
  const page = await app.firstWindow();
  await page.waitForSelector("nav", { timeout: 40000 });
  return { app, page };
}
const menu = (app) => app.evaluate(() => globalThis.kakuregaShell.menu().items.map((i) => ({ id: i.id, label: i.label, sub: i.submenu?.items.map((s) => ({ label: s.label, checked: s.checked })) })));
const signer = (page) => page.evaluate(async () => (await (await fetch("/api/config")).json()).signer);

let app;
try {
  let page;
  ({ app, page } = await launch());
  check("with one profile the menu offers no choice of profile", !(await menu(app)).some((i) => i.id === "profile"));
  check("the first profile works in the folder itself and shows no profile name", existsSync(join(home, "admin.json")) && (await page.locator(".badges").innerText()).includes("Profile") === false);
  const firstSigner = await signer(page);
  const made = spawnSync(kaku, ["profile", "new", "work"], { env, encoding: "utf8" });
  check("a profile made in the terminal", made.status === 0, made.stderr);
  // the menu is rebuilt when something on it changes; a poll of the inbox is such a moment
  await app.evaluate(() => globalThis.kakuregaShell.pollInbox());
  const item = (await menu(app)).find((i) => i.id === "profile");
  check("appears in the menu, with the one in use marked", item?.label === "Profile: default" && JSON.stringify(item.sub) === JSON.stringify([{ label: "default", checked: true }, { label: "work", checked: false }]), JSON.stringify(item));
  const closed = new Promise((r) => app.process().once("exit", r));
  await app.evaluate(() => globalThis.kakuregaShell.menu().getMenuItemById("profile-work").click()).catch(() => {});
  await Promise.race([closed, new Promise((r) => setTimeout(r, 15000))]);
  check("choosing it saves the choice and ends the app and its companion", readFileSync(join(home, "current-profile"), "utf8") === "work\n" && app.process().exitCode !== null && !existsSync(join(home, "admin.json")), String(app.process().exitCode));

  ({ app, page } = await launch());
  const badges = await page.locator(".badges").innerText();
  check("the next start works in that profile's own folder and says so", existsSync(join(home, "profiles/work/admin.json")) && !existsSync(join(home, "admin.json")) && badges.includes("Profile: work"), badges);
  await page.screenshot({ path: join(work, "work-profile.png") });
  check("with its own signing key", (await signer(page)) !== firstSigner);
  check("and keeps the shell's own state inside that folder", existsSync(join(home, "profiles/work/electron")));
  const now = (await menu(app)).find((i) => i.id === "profile");
  check("the menu marks it as the one in use", now?.label === "Profile: work" && now.sub.find((s) => s.label === "work")?.checked === true, JSON.stringify(now));
} catch (e) {
  check("the run finished without an error", false, String(e?.stack ?? e).slice(0, 500));
} finally {
  await app?.close().catch(() => {});
}
console.log(`\n${passed}/${passed + failed} checks passed`);
process.exit(failed ? 1 : 0);
