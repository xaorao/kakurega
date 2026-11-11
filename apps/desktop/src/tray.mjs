import { deflateSync } from "node:zlib";

// The menu-bar item: its icon, the words next to it and its menu. Everything here is plain data so it can be tested without
// a screen; main.mjs turns it into Electron's Tray and Menu.

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

/** Coverage (0 to 1) of the icon at a point of a 16-unit square: a rounded frame with a solid bar across it, the redaction mark. */
function coverage(x, y) {
  const inside = (x0, y0, x1, y1, r) => {
    const cx = Math.min(Math.max(x, x0 + r), x1 - r);
    const cy = Math.min(Math.max(y, y0 + r), y1 - r);
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
  };
  const frame = inside(1.5, 1.5, 14.5, 14.5, 3) && !inside(3, 3, 13, 13, 1.6);
  const bar = x >= 4.5 && x <= 11.5 && y >= 6.75 && y <= 9.25;
  return frame || bar;
}

/**
 * The icon as a PNG of `size` pixels square: black with transparency, which macOS treats as a template image and draws in
 * the menu bar's own colour, light or dark. Each pixel is sampled 4 x 4 times so the edges are smooth.
 */
export function trayIconPng(size) {
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let py = 0; py < size; py++) {
    raw[py * (size * 4 + 1)] = 0;
    for (let px = 0; px < size; px++) {
      let hit = 0;
      for (let sy = 0; sy < 4; sy++) for (let sx = 0; sx < 4; sx++) if (coverage(((px + (sx + 0.5) / 4) * 16) / size, ((py + (sy + 0.5) / 4) * 16) / size)) hit++;
      const o = py * (size * 4 + 1) + 1 + px * 4;
      raw[o + 3] = Math.round((hit / 16) * 255);
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header[8] = 8;
  header[9] = 6;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", header), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

/** The words next to the icon (macOS): the number waiting, or nothing. */
export function trayTitle(count) {
  return Number.isInteger(count) && count > 0 ? String(count) : "";
}

/** The status line: running and how much waits, or that the companion has stopped for good. */
export function statusLine({ count, stopped }, t) {
  if (stopped) return t("tray.stopped");
  if (!Number.isInteger(count) || count <= 0) return t("tray.nothing");
  return count === 1 ? t("tray.waiting.one") : t("tray.waiting.many", { n: count });
}

export function trayToolTip(state, t) {
  return state.stopped ? t("tray.stopped") : `${t("tray.running")} · ${statusLine(state, t)}`;
}

/**
 * The menu, as an Electron menu template. `actions` holds the four things the menu can do; the status lines are not
 * clickable. Every entry has an id so a test or the owner's assistive technology can find it.
 */
export function menuTemplate(state, t, actions) {
  const n = Number.isInteger(state.count) && state.count > 0 ? state.count : 0;
  return [
    { id: "running", label: state.stopped ? t("tray.stopped") : t("tray.running"), enabled: false },
    ...(state.stopped ? [] : [{ id: "status", label: statusLine(state, t), enabled: false }]),
    { type: "separator" },
    { id: "open", label: t("tray.open"), click: () => actions.open() },
    { id: "inbox", label: n > 0 ? t("tray.inboxCount", { n }) : t("tray.inbox"), click: () => actions.inbox() },
    ...(state.stopped || !actions.capture ? [] : [{ id: "capture", label: t("tray.capture"), click: () => actions.capture() }]),
    { type: "separator" },
    { id: "notify", label: t("tray.notify"), type: "checkbox", checked: !!state.notify, click: (item) => actions.setNotify(!!item?.checked) },
    { type: "separator" },
    { id: "quit", label: t("tray.quit"), click: () => actions.quit() },
  ];
}
