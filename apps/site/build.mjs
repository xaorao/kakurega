// Puts the public site together in apps/site/dist: the landing page, the one-file check page as /verify/, and a real
// capsule to try it on. Nothing here needs a server: every file is static.
//   node apps/site/build.mjs
import { cpSync, mkdirSync, rmSync, existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const out = join(here, "dist");
const built = spawnSync("node", ["build.mjs"], { cwd: join(repo, "apps/verifier-page"), encoding: "utf8" });
if (built.status !== 0) throw new Error("the check page did not build: " + built.stderr);
rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, "verify"), { recursive: true });
mkdirSync(join(out, "sample"), { recursive: true });
for (const f of ["index.html", "site.css"]) cpSync(join(here, f), join(out, f));
cpSync(join(here, "img"), join(out, "img"), { recursive: true });
cpSync(join(repo, "apps/verifier-page/dist/kakurega-verify.html"), join(out, "verify/index.html"));
const capsule = join(repo, "fixtures/near-live/review.kkg");
if (!existsSync(capsule)) throw new Error("the sample capsule is missing: " + capsule);
cpSync(capsule, join(out, "sample/review.kkg"));
console.log("site built in " + out);
