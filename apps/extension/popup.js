import { call, parsePort } from "./lib.js";

const $ = (id) => document.getElementById(id);

async function config() {
  const { port, token } = await chrome.storage.local.get(["port", "token"]);
  return { port, token };
}

async function render() {
  $("error").textContent = "";
  const c = await config();
  $("pair").hidden = !!c.token;
  $("connected").hidden = !c.token;
  if (!c.token) {
    $("state").className = "";
    $("state").textContent = "Not connected.";
    return;
  }
  try {
    const r = await call(fetch, c, "GET", "/ext/status");
    if (r.status === 200) {
      $("state").className = "ok";
      $("state").textContent = `Connected. ${r.body.pending_captures} capture(s) waiting for your review.`;
    } else {
      $("state").className = "";
      $("state").textContent = "The pairing is no longer valid.";
      await chrome.storage.local.remove(["token"]);
      $("pair").hidden = false;
      $("connected").hidden = true;
    }
  } catch {
    $("state").className = "";
    $("state").textContent = "Kakurega is not running on this computer.";
  }
}

$("pair").addEventListener("submit", async (e) => {
  e.preventDefault();
  $("error").textContent = "";
  const port = parsePort($("port").value);
  const code = $("code").value.trim();
  if (port === null || !/^[0-9a-f]{12}$/.test(code)) {
    $("error").textContent = "Enter the port and the 12-character code.";
    return;
  }
  try {
    const r = await call(fetch, { port }, "POST", "/ext/pair", { code });
    if (r.status !== 200 || typeof r.body?.token !== "string") {
      $("error").textContent = r.body?.error?.message ?? `Pairing failed (${r.status}).`;
      return;
    }
    await chrome.storage.local.set({ port, token: r.body.token });
    $("code").value = "";
    await render();
  } catch {
    $("error").textContent = "Kakurega is not running on that port.";
  }
});

$("forget").addEventListener("click", async () => {
  await chrome.storage.local.remove(["token", "port"]);
  await render();
});

render();
