// One picture of the screen, taken by the shell and handed to the chat as an attachment.
//
// The page never asks for this and never sees the pixels: every permission request from the page stays refused. The owner
// starts it from the shell's own menu, the picture is taken in the main process, the owner is shown it and says yes or
// no, and only on yes do the bytes go to the service on this computer, through the same attachment door a dropped file
// uses (size limits, reading the text out of the picture, withholding lines that hold a key). Nothing is kept by the shell.

/** The largest picture the service takes as one attachment. A bigger one is not sent. */
export const MAX_PICTURE_BYTES = 8 * 1024 * 1024;

const ATTACHMENT_ID = /^att-[0-9a-f]{12}$/;

/** `screen-20260102-030405.png`, from the local time given. */
export function pictureName(when = new Date()) {
  const p = (n) => String(n).padStart(2, "0");
  return `screen-${when.getFullYear()}${p(when.getMonth() + 1)}${p(when.getDate())}-${p(when.getHours())}${p(when.getMinutes())}${p(when.getSeconds())}.png`;
}

/** The screen source for the display the owner is on; the first one when it cannot be told. */
export function pickSource(sources, displayId) {
  if (!Array.isArray(sources) || sources.length === 0) return null;
  return sources.find((s) => String(s.display_id ?? "") === String(displayId)) ?? sources[0];
}

/** The address of the chat screen that picks this attachment up, or null for an id that is not an attachment's. */
export function chatTarget(id) {
  return typeof id === "string" && ATTACHMENT_ID.test(id) ? `#/chat/attach/${id}` : null;
}

/**
 * `grab()` gives the picture as PNG bytes (or null), `confirm(png)` asks the owner, `upload(name, png)` hands it to the
 * service and answers `{ status, json }`, `open(hash)` shows the chat. Resolves to what happened, in one word.
 */
export function createCapture({ grab, confirm, upload, open, now = () => new Date() }) {
  let running = false;
  return async function capture() {
    // one at a time: a second choice of the menu item while the first picture waits for its answer does nothing
    if (running) return { outcome: "busy" };
    running = true;
    try {
      const png = await grab();
      if (!png || png.length === 0) return { outcome: "no_picture" };
      if (png.length > MAX_PICTURE_BYTES) return { outcome: "too_large" };
      if (!(await confirm(png))) return { outcome: "discarded" };
      const r = await upload(pictureName(now()), png);
      const target = r?.status === 200 || r?.status === 201 ? chatTarget(r.json?.id) : null;
      if (!target) return { outcome: "refused", status: r?.status ?? 0 };
      open(target);
      return { outcome: "attached", id: r.json.id };
    } finally {
      running = false;
    }
  };
}
