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
      $("state").textContent = `Connected. ${r.body.pending_captures} ${r.body.pending_captures === 1 ? "capture" : "captures"} waiting for your review.`;
    } else {
      $("state").className = "";
      $("state").textContent = "The pairing is no longer valid.";
      await chrome.storage.local.remove(["token"]);
      $("pair").hidden = false;
      $("connected").hidden = true;
    }
  } catch {
    $("state").className = "";
    // nothing answered at the address saved when this browser was paired: the service is stopped, or it runs at
    // another address now (it keeps its address across restarts when it can)
    $("state").textContent = `Nothing answers at port ${c.port} on this computer. Start Kakurega; if it is running, it has another address now: disconnect this browser and pair again.`;
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
      // the service's own sentence, as a sentence; an answer that is not Kakurega's is said as that
      const said = r.body?.error?.message;
      $("error").textContent = said ? said.charAt(0).toUpperCase() + said.slice(1) + (/[.!?]$/.test(said) ? "" : ".") : r.status === 200 ? "Something answered on that port, but it is not Kakurega. Check the port." : `Kakurega refused the pairing (${r.status}).`;
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
  // Kakurega is told too, so its side no longer says an extension is paired; if it is not running, the token is
  // forgotten here all the same
  let told = true;
  try {
    const saved = await chrome.storage.local.get(["token", "port"]);
    if (saved.token && saved.port) await call(fetch, saved, "POST", "/ext/unpair", {});
  } catch {
    // not reachable: this browser forgets its side, and the person is told that Kakurega's side is still there
    told = false;
  }
  await chrome.storage.local.remove(["token", "port"]);
  await render();
  if (!told) $("state").textContent = "Disconnected in this browser. Kakurega was not running, so it still lists this browser as paired. To remove it there too, run `kaku extension revoke` in a terminal, or pair again (a new pairing replaces the old one).";
});

render();
