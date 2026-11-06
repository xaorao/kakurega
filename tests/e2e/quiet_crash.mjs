import { spawnSync } from "node:child_process";

// Tests crash or kill the app on purpose; macOS would show "quit unexpectedly" each time. Hidden for this run only, and
// the owner's own setting is put back when the script ends.
const key = "com.apple.CrashReporter";
if (process.platform === "darwin") {
  const before = spawnSync("defaults", ["read", key, "DialogType"], { encoding: "utf8" });
  spawnSync("defaults", ["write", key, "DialogType", "none"]);
  process.on("exit", () => {
    if (before.status === 0) spawnSync("defaults", ["write", key, "DialogType", before.stdout.trim()]);
    else spawnSync("defaults", ["delete", key, "DialogType"]);
  });
}
