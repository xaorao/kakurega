// Reports whether it is running inside Electron or as plain Node, without opening any window.
const fs = require("node:fs");
const inElectron = !!process.versions.electron;
fs.writeFileSync(process.env.PROBE_OUT, String(inElectron));
if (inElectron) {
  const { app } = require("electron");
  app.dock?.hide?.();
  app.quit();
}
