import { createRequire } from "node:module";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(resolve(here, "../web/package.json"));
const { build } = await import(require.resolve("vite"));

const result = await build({
  root: here,
  logLevel: "warn",
  configFile: false,
  resolve: {
    alias: {
      "@kakurega/verifier": resolve(here, "../../packages/verifier-js/src/index.js"),
      "@kakurega/canon": resolve(here, "../../packages/canon-js/src/index.js"),
    },
  },
  build: {
    write: false,
    minify: true,
    target: "es2022",
    lib: { entry: resolve(here, "main.js"), formats: ["iife"], name: "KakuregaCheck", fileName: "check" },
  },
});
const out = (Array.isArray(result) ? result[0] : result).output.find((o) => o.type === "chunk");
const js = out.code.replace(/<\/script/gi, "<\\/script");
const html = readFileSync(resolve(here, "page.html"), "utf8").replace("/*BUNDLE*/", () => js);
mkdirSync(resolve(here, "dist"), { recursive: true });
writeFileSync(resolve(here, "dist/kakurega-verify.html"), html);
console.log(`dist/kakurega-verify.html ${html.length} bytes`);
