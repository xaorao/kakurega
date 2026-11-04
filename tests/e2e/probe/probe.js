// Reports whether it is running inside Electron or as plain Node, without opening any window.
const fs = require("node:fs");
// process.versions.electron is set even when Electron runs as a plain Node interpreter; process.type is only set in a real app
const inElectron = process.type === "browser";
fs.writeFileSync(process.env.PROBE_OUT, String(inElectron));
if (inElectron) {
  const { app } = require("electron");
  app.dock?.hide?.();
  app.quit();
}
