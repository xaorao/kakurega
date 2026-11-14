import test from "node:test";
import assert from "node:assert/strict";
import { baseUrl, call, cleanSelection, describeFailure, pageOf, parsePort } from "../lib.js";
import { captureSelection } from "../capture.js";

test("ex01 only a plain loopback port is accepted, so a stored value can never point elsewhere", () => {
  assert.equal(parsePort("8080"), 8080);
  assert.equal(parsePort(" 54321 "), 54321);
  for (const bad of ["80", "70000", "8080.1", "8e3", "-1", "", "evil.com:80", "8080/x", "0x1f90", null, undefined, {}]) assert.equal(parsePort(bad), null, String(bad));
  assert.equal(baseUrl("8080"), "http://127.0.0.1:8080");
  assert.equal(baseUrl("nope"), null);
});

test("ex02 a selection is trimmed, bounded and refused if it hides control or direction characters", () => {
  assert.equal(cleanSelection("  hello\r\nworld  "), "hello\nworld");
  for (const bad of ["", "   ", "a\u0000b", "x\u001b[2Jy", "evil ‮txt", "a⁦b", "x".repeat(2001), 5, null]) assert.equal(cleanSelection(bad), null, JSON.stringify(bad));
  assert.equal(cleanSelection("x".repeat(2000)).length, 2000);
  assert.equal(cleanSelection("tab\tallowed"), "tab\tallowed");
});

test("ex03 only the origin of an http or https page is ever sent, never the path, query or credentials", () => {
  assert.equal(pageOf("https://Wiki.Example.com/a/b?q=secret#frag"), "https://wiki.example.com/");
  assert.equal(pageOf("http://localhost:3000/x"), "http://localhost:3000/");
  assert.equal(pageOf("https://user:pw@host.example/"), "https://host.example/");
  for (const bad of ["file:///etc/passwd", "chrome://settings", "javascript:alert(1)", "data:text/html,x", "about:blank", "not a url", "", undefined]) assert.equal(pageOf(bad), null, String(bad));
});

test("ex04 requests carry the token, never cookies, never follow redirects and go only to loopback", async () => {
  const seen = [];
  const fakeFetch = async (url, init) => {
    seen.push([url, init]);
    return { status: 200, json: async () => ({ ok: true }) };
  };
  const r = await call(fakeFetch, { port: 8080, token: "T" }, "POST", "/ext/capture", { a: 1 });
  assert.equal(r.status, 200);
  const [url, init] = seen[0];
  assert.equal(url, "http://127.0.0.1:8080/ext/capture");
  assert.equal(init.headers.Authorization, "Bearer T");
  assert.equal(init.credentials, "omit");
  assert.equal(init.redirect, "error");
  assert.equal(init.cache, "no-store");
  await assert.rejects(call(fakeFetch, { port: "evil.com", token: "T" }, "GET", "/ext/status"), /No valid port/);
  assert.equal(seen.length, 1, "nothing was sent for an invalid port");
});

test("ex05 captureSelection reports every outcome in plain words and sends only text and origin", async () => {
  const cfg = { port: 9000, token: "T" };
  const ok = async (url, init) => ({ status: 202, json: async () => ({ queued: true }) });
  let body;
  const spy = async (url, init) => { body = JSON.parse(init.body); return ok(); };
  const r = await captureSelection({ selectionText: " Acme pays on the 20th ", pageUrl: "https://wiki.example.com/p?x=1" }, { fetch: spy, config: cfg });
  assert.equal(r.ok, true);
  assert.deepEqual(body, { text: "Acme pays on the 20th", page: "https://wiki.example.com/" });
  assert.deepEqual(Object.keys(body).sort(), ["page", "text"]);
  assert.equal((await captureSelection({ selectionText: "x", pageUrl: "https://a.example/" }, { fetch: ok, config: {} })).ok, false, "not paired");
  assert.match((await captureSelection({ selectionText: "", pageUrl: "https://a.example/" }, { fetch: ok, config: cfg })).message, /Nothing is selected/);
  // each reason is named, since each is fixed in a different way
  assert.match((await captureSelection({ selectionText: "x".repeat(2001), pageUrl: "https://a.example/" }, { fetch: ok, config: cfg })).message, /too long to send: it has 2001 characters and the most is 2000/);
  assert.match((await captureSelection({ selectionText: "evil \u202etxt", pageUrl: "https://a.example/" }, { fetch: ok, config: cfg })).message, /hidden characters/);
  assert.match((await captureSelection({ selectionText: "x", pageUrl: "chrome://x" }, { fetch: ok, config: cfg })).message, /http or https/);
  const down = async () => { throw new TypeError("failed to fetch"); };
  assert.match((await captureSelection({ selectionText: "x", pageUrl: "https://a.example/" }, { fetch: down, config: cfg })).message, /Nothing answers at Kakurega.s saved address/);
  const refused = async () => ({ status: 422, json: async () => ({ error: { message: "looks like a secret" } }) });
  assert.match((await captureSelection({ selectionText: "x", pageUrl: "https://a.example/" }, { fetch: refused, config: cfg })).message, /looks like a secret/i);
  const unpaired = async () => ({ status: 401, json: async () => ({}) });
  assert.match((await captureSelection({ selectionText: "x", pageUrl: "https://a.example/" }, { fetch: unpaired, config: cfg })).message, /pair again/);
  assert.match(describeFailure(429), /wait a minute/i);
});

test("ex10 the background worker wires one context-menu entry on selections and reports the outcome on the toolbar button", async () => {
  const listeners = { installed: [], clicked: [] };
  const calls = [];
  globalThis.chrome = {
    runtime: { onInstalled: { addListener: (f) => listeners.installed.push(f) } },
    contextMenus: { removeAll: (cb) => cb(), create: (m) => calls.push(["create", m]), onClicked: { addListener: (f) => listeners.clicked.push(f) } },
    action: { setBadgeText: async (b) => calls.push(["badge", b.text]), setTitle: async (t) => calls.push(["title", t.title]) },
    storage: { local: { get: async () => ({ port: 9100, token: "T" }) } },
  };
  const sent = [];
  globalThis.fetch = async (url, init) => { sent.push([url, JSON.parse(init.body)]); return { status: 202, json: async () => ({ queued: true }) }; };
  await import("../background.js");
  listeners.installed[0]();
  const create = calls.find((c) => c[0] === "create")[1];
  assert.deepEqual(create.contexts, ["selection"]);
  assert.equal(create.title, "Send selection to Kakurega");
  await listeners.clicked[0]({ menuItemId: "something-else", selectionText: "x", pageUrl: "https://a.example/" });
  assert.equal(sent.length, 0, "a click on any other menu item sends nothing");
  await listeners.clicked[0]({ menuItemId: create.id, selectionText: "Acme pays on the 20th", pageUrl: "https://wiki.example.com/page?q=1" });
  assert.deepEqual(sent, [["http://127.0.0.1:9100/ext/capture", { text: "Acme pays on the 20th", page: "https://wiki.example.com/" }]]);
  assert.ok(calls.some((c) => c[0] === "badge" && c[1] === "OK") && calls.some((c) => c[0] === "title" && /Queued/.test(c[1])));
});
