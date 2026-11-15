import { modelDown, setModelDown } from "./modelState";
import { Opening } from "./components/Opening";
import { Glyph } from "./components/Glyph";
import { createEffect, createMemo, createSignal, ErrorBoundary, For, lazy, onCleanup, onMount, Show, Suspense } from "solid-js";
import { api, ApiError, setCsrf, type Config, type Job, type KeptChat, stateOf } from "./api";
void ApiError;
import { NewReview } from "./screens/NewReview";
import { ReviewView } from "./screens/ReviewView";
// every screen but the first one is loaded when it is first opened, from this program only
// They are not put under one waiting boundary: that boundary would take the whole screen away, and the keyboard focus with
// it, every time any part of a screen reads something again.
const Handoff = lazy(() => import("./screens/Handoff").then((m) => ({ default: m.Handoff })));
const Projects = lazy(() => import("./screens/Projects").then((m) => ({ default: m.Projects })));
const Skills = lazy(() => import("./screens/Skills").then((m) => ({ default: m.Skills })));
const Verify = lazy(() => import("./screens/Verify").then((m) => ({ default: m.Verify })));
const Memory = lazy(() => import("./screens/Memory").then((m) => ({ default: m.Memory })));
const Instructions = lazy(() => import("./screens/Instructions").then((m) => ({ default: m.Instructions })));
const Settings = lazy(() => import("./screens/Settings").then((m) => ({ default: m.Settings })));
const Activity = lazy(() => import("./screens/Activity").then((m) => ({ default: m.Activity })));
const AgentAccess = lazy(() => import("./screens/AgentAccess").then((m) => ({ default: m.AgentAccess })));
const Privacy = lazy(() => import("./screens/Privacy").then((m) => ({ default: m.Privacy })));
const Chat = lazy(() => import("./screens/Chat").then((m) => ({ default: m.Chat })));
const Roundtable = lazy(() => import("./screens/Roundtable").then((m) => ({ default: m.Roundtable })));
const Team = lazy(() => import("./screens/Team").then((m) => ({ default: m.Team })));
const Documents = lazy(() => import("./screens/Documents").then((m) => ({ default: m.Documents })));
const Meeting = lazy(() => import("./screens/Meeting").then((m) => ({ default: m.Meeting })));
const BreakIt = lazy(() => import("./screens/BreakIt").then((m) => ({ default: m.BreakIt })));
const CodeTask = lazy(() => import("./screens/CodeTask").then((m) => ({ default: m.CodeTask })));
const Explore = lazy(() => import("./screens/Explore").then((m) => ({ default: m.Explore })));
const Inbox = lazy(() => import("./screens/Inbox").then((m) => ({ default: m.Inbox })));
import { notifier } from "./notifier";
import { Palette, type PaletteItem } from "./components/Palette";
import { Welcome } from "./components/Welcome";
import { welcomeDone } from "./welcome";
import { ShortcutHelp } from "./components/ShortcutHelp";
import { appShortcut } from "./shortcuts";
import { singleKeys } from "./single-keys";
import { createLive } from "./live";
import { clearDrafts } from "./composer";
import { applyLocale, LOCALES, locale, rich, setLocale, t, type Locale } from "./i18n";

function useHash() {
  const [hash, setHash] = createSignal(location.hash);
  const on = () => {
    // another screen opens at its top, not where the last one was scrolled to; a link to a part of the same screen
    // (#pv-model) moves the page itself
    const screen = (h: string) => h.split("/").slice(0, 2).join("/");
    if (screen(location.hash) !== screen(hash())) window.scrollTo(0, 0);
    setHash(location.hash);
  };
  window.addEventListener("hashchange", on);
  onCleanup(() => window.removeEventListener("hashchange", on));
  return hash;
}

import { applyTheme, chooseTheme, theme, type Theme } from "./theme";
import { learnHome } from "./components/ChosenFolder";

function ThemePicker() {
  onMount(() => applyTheme(theme()));
  const choose = chooseTheme;
  return (
    <label class="theme">
      <span class="sr">{t("theme.label")}</span>
      <select value={theme()} onChange={(e) => choose(e.currentTarget.value as Theme)}>
        <option value="system">{t("theme.system")}</option>
        <option value="light">{t("theme.light")}</option>
        <option value="dark">{t("theme.dark")}</option>
      </select>
    </label>
  );
}

function LanguagePicker() {
  return (
    <label class="theme">
      <span class="sr">{t("lang.label")}</span>
      <select value={locale()} onChange={(e) => setLocale(e.currentTarget.value as Locale)}>
        <For each={LOCALES}>{([code, name]) => <option value={code}>{name}</option>}</For>
      </select>
    </label>
  );
}

function Pair(props: { onDone: () => void }) {
  const [code, setCode] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const submit = async (value: string) => {
    setBusy(true);
    setError("");
    try {
      const r = await api.pair(value.trim());
      setCsrf(r.csrf);
      history.replaceState(null, "", location.pathname);
      props.onDone();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("pair.unreachable"));
    } finally {
      setBusy(false);
    }
  };
  // a pairing link opened in this tab, now or later (a fresh link after the first one ran out), is used as it arrives
  const fromLink = () => /#pair=([A-Za-z0-9_-]+)/.exec(location.hash)?.[1];
  const first = fromLink();
  if (first) queueMicrotask(() => submit(first));
  const onHash = () => { const c = fromLink(); if (c) void submit(c); };
  window.addEventListener("hashchange", onHash);
  onCleanup(() => window.removeEventListener("hashchange", onHash));
  return (
    <main class="center pair">
      <span class="brand-mark big" aria-hidden="true"><Glyph /></span>
      <h1>{t("pair.title")}</h1>
      <p class="muted">{rich("pair.intro")}</p>
      <form
        class="stack"
        onSubmit={(e) => {
          e.preventDefault();
          // the whole link pasted into the box works as well as the code inside it
          submit(/#pair=([A-Za-z0-9_-]+)/.exec(code())?.[1] ?? code().trim());
        }}
      >
        <label for="code">{t("pair.label")}</label>
        <input id="code" autocomplete="off" spellcheck={false} value={code()} onInput={(e) => setCode(e.currentTarget.value)} />
        <button type="submit" disabled={busy() || code().trim().length === 0}>{t("pair.button")}</button>
        <Show when={error()}>
          <p role="alert" class="bad">{error()}</p>
        </Show>
      </form>
    </main>
  );
}

/** The service names the sandbox in English; the page says it in the owner's language. A wording it does not know is shown as it came. */
function sandboxWords(transport: string | undefined): string {
  if (!transport) return t("badge.sandbox.local");
  if (transport === "local sandbox") return t("badge.sandbox.local");
  const vm = /^sandbox in the (.+) virtual machine$/.exec(transport)?.[1];
  return vm ? t("badge.sandbox.vm", { vm }) : transport;
}

/** The two standing safeguards, said quietly at the foot of the sidebar: how the sandbox is sealed and who approves a release. */
function Safeguards(props: { config: Config }) {
  const r = () => props.config.route;
  return (
    <div class="safeguards">
      <span class="badge" title={`${t("badge.sandbox.title")} (${r().transport ?? ""})`}>
        {sandboxWords(r().transport)}
      </span>
      <Show when={props.config.approvals?.asks_every_release}>
        <span class="badge" title={t(props.config.approvals?.signing_key_enrolled ? "badge.approvals.signed.title" : "badge.approvals.title")}>
          {t(props.config.approvals?.signing_key_enrolled ? "badge.approvals.signed" : "badge.approvals")}
        </span>
      </Show>
    </div>
  );
}

/** A sidebar link's small line drawing; decoration only, the link's words are its name. */
function NavIcon(props: { d: string }) {
  return (
    <svg class="nav-icon" width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d={props.d} />
    </svg>
  );
}

function Badge(props: { config: Config; modelDown: boolean }) {
  const r = () => props.config.route;
  // the local model's check speaks only for the badge's model when that model is the one on this computer
  const down = () => props.modelDown && r().stays_here !== false && !r().simulated;
  return (
    <div class="badges">
      <Show
        when={r().stays_here !== false || r().simulated}
        fallback={
          <Show
            when={r().class === "verified_tee"}
            fallback={
              <span class="badge warn" title={t("badge.remote.title", { host: r().host ?? "" })}>
                {r().host ? t("badge.remote", { host: r().host ?? "" }) : t("badge.unknown")}
              </span>
            }
          >
            {/* off this computer, but on hardware whose proof is checked before each question, answers signed by the model */}
            <span class="badge ok" id="badge-tee" title={t("badge.tee.title", { host: r().host ?? "" })}>
              <span class="dot" /> {t("badge.tee")}
            </span>
          </Show>
        }
      >
        <span class={down() ? "badge warn" : "badge ok"} title={down() ? t("badge.model.down.title") : t("badge.private.title")}>
          <span class="dot" /> {t("badge.private")}
        </span>
      </Show>
      <span class={down() ? "badge warn" : "badge"} title={down() ? t("badge.model.down.title") : undefined}>
        {r().model === "scripted test model" ? t("badge.model.scripted") : (r().model ?? r().id)}
        <Show when={down()}> · {t("badge.model.down")}</Show>
      </span>
      <Show when={props.config.profile}>
        <span class="badge">{t("head.profile", { name: props.config.profile ?? "" })}</span>
      </Show>
    </div>
  );
}

export function App() {
  if (location.pathname === "/verify") return <Suspense fallback={<p class="muted" role="status">{t("review.loading")}</p>}><Verify /></Suspense>;
  const hash = useHash();
  const [paired, setPaired] = createSignal<boolean | null>(null);
  const [config, setConfig] = createSignal<Config | null>(null);
  // Signed out by any path (the button, the palette, an expired session): nothing typed stays in the page.
  createEffect(() => {
    if (paired() === false) clearDrafts();
  });
  const [jobs, setJobs] = createSignal<Job[]>([]);
  const [query, setQuery] = createSignal("");
  const [unreachable, setUnreachable] = createSignal(false);
  const [failures, setFailures] = createSignal(0);
  const [waiting, setWaiting] = createSignal(0);
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const [helpOpen, setHelpOpen] = createSignal(false);
  const [welcome, setWelcome] = createSignal(!welcomeDone(typeof localStorage === "undefined" ? undefined : localStorage));
  {
    const open = () => setHelpOpen(true);
    window.addEventListener("kk:help", open);
    onCleanup(() => window.removeEventListener("kk:help", open));
  }
  // on a narrow screen the menu is closed until asked for; choosing a page closes it again
  const [menuOpen, setMenuOpen] = createSignal(false);
  // null: open exactly when one of its screens is shown; true or false once the owner has opened or closed it by hand
  const [moreOpen, setMoreOpen] = createSignal<boolean | null>(null);
  const inMore = () => hash() === "#/meeting" || hash() === "#/explore" || hash() === "#/code" || hash() === "#/roundtable" || hash() === "#/handoff" || hash() === "#/instructions" || hash() === "#/skills" || hash() === "#/break" || hash() === "#/agent" || hash() === "#/activity";
  createEffect(() => { hash(); setMenuOpen(false); });
  // the desktop shell opens this address after the owner took a picture of the screen: the attachment is handed to the chat once
  createEffect(() => {
    const id = /^#\/chat\/attach\/(att-[0-9a-f]{12})$/.exec(hash())?.[1];
    if (!id) return;
    void api.attachment(id)
      .then((a) => {
        const { read: _read, ...summary } = a;
        try { sessionStorage.setItem("kk-chat-attach", JSON.stringify(summary)); } catch { /* the chat opens without it */ }
      })
      .catch(() => { /* a file that is gone: the chat opens without it */ })
      .finally(() => { location.hash = "#/chat"; });
  });
  const shown = createMemo(() => {
    const q = query().trim().toLowerCase();
    if (!q) return jobs();
    return jobs().filter((j) => `${j.repo} ${j.recipient} ${j.state.replace(/_/g, " ")} ${t(`state.${stateOf(j)}` as never)}`.toLowerCase().includes(q));
  });
  const onKey = (e: KeyboardEvent) => {
    if (e.isComposing || e.defaultPrevented) return;
    const found = appShortcut(e, e.target as HTMLElement | null, singleKeys());
    if (!found) return;
    e.preventDefault();
    if (found.id === "palette") {
      if (paired()) setPaletteOpen(!paletteOpen());
    } else if (found.id === "help" || found.id === "help-anywhere") {
      if (paired() && !paletteOpen()) setHelpOpen(!helpOpen());
    }
  };
  const showHelp = () => {
    if (paired() && !paletteOpen()) setHelpOpen(true);
  };
  window.addEventListener("kk-show-shortcuts", showHelp);
  onCleanup(() => window.removeEventListener("kk-show-shortcuts", showHelp));
  applyLocale();
  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));

  // a setting that changes what the header says about the route (the model) asks for the configuration again
  const configStale = () => void api.config().then((c) => { learnHome(c.home); setConfig(c); }).catch(() => {});
  window.addEventListener("kk-config-stale", configStale);
  onCleanup(() => window.removeEventListener("kk-config-stale", configStale));

  // any request that found the service gone, or found that it no longer knows this browser, brings the app to the one
  // screen that says so and what to do, whatever screen was open
  const gone = () => {
    if (paired() !== true) return;
    setUnreachable(true);
    setPaired(false);
  };
  const forgotten = () => {
    if (paired() === true) setPaired(false);
  };
  window.addEventListener("kk-unreachable", gone);
  window.addEventListener("kk-unpaired", forgotten);
  onCleanup(() => {
    window.removeEventListener("kk-unreachable", gone);
    window.removeEventListener("kk-unpaired", forgotten);
  });

  const load = async () => {
    try {
      const s = await api.session();
      if (!s.paired) {
        setPaired(false);
        return;
      }
      setCsrf(s.csrf);
      const [c, j] = await Promise.all([api.config(), api.jobs()]);
      learnHome(c.home);
      setConfig(c);
      setJobs(j.jobs);
      setPaired(true);
      // what waits for the owner is counted at once, not at the first ten-second look
      void api.inbox().then((inbox) => setWaiting(inbox?.count ?? 0)).catch(() => {});
    } catch (e) {
      // A refused answer from the service means "not paired"; no answer at all means the service is not there.
      if (e instanceof ApiError) setPaired(false);
      else {
        setUnreachable(true);
        setPaired(false);
      }
    }
  };
  createEffect(() => {
    if (paired() === null) void load();
  });
  const refresh = async () => {
    try {
      setJobs((await api.jobs()).jobs);
      setFailures(0);
      const inbox = await api.inbox().catch(() => null);
      setWaiting(inbox?.count ?? 0);
      if (inbox) void notifier.observe(inbox.items);
    } catch {
      // keep the last state on screen, say so after three misses in a row, and keep trying
      setFailures((n) => n + 1);
    }
  };
  // Changes arrive as server-sent events; the timer is only a slow safety net in case the stream is unavailable.
  const timer = setInterval(() => paired() && void refresh(), 10000);
  onCleanup(() => clearInterval(timer));
  const live =
    typeof EventSource === "undefined"
      ? null
      : createLive({
          open: () => new EventSource("/api/events"),
          onChange: () => {
            void refresh();
            window.dispatchEvent(new Event("kk-changed"));
          },
        });
  const watch = setInterval(() => live?.check(), 1000);
  const onVisible = () => {
    if (document.visibilityState === "visible") live?.foreground();
  };
  const onOnline = () => live?.online();
  document.addEventListener("visibilitychange", onVisible);
  window.addEventListener("online", onOnline);
  // whether the model on this computer answers, from the readiness checks: read at load, when the page comes back to the
  // front (at most once a minute) and after a reply fails
  let modelChecked = 0;
  const checkModel = async (force = false) => {
    if (!paired() || (!force && Date.now() - modelChecked < 60_000)) return;
    modelChecked = Date.now();
    try {
      const r = await api.readiness();
      setModelDown(r.checks.some((c) => (c.id === "ollama" || c.id === "model") && !c.ok));
    } catch {
      // the service did not answer: that is said elsewhere
    }
  };
  createEffect(() => {
    if (paired()) void checkModel(true);
  });
  const onModelCheck = () => void checkModel(true);
  // a reply came from the model, so it answers
  const onModelOk = () => setModelDown(false);
  window.addEventListener("kk:model-check", onModelCheck);
  window.addEventListener("kk:model-ok", onModelOk);
  onCleanup(() => {
    window.removeEventListener("kk:model-check", onModelCheck);
    window.removeEventListener("kk:model-ok", onModelOk);
  });
  const onFocus = () => {
    void notifier.focusChanged();
    if (document.hasFocus()) void checkModel();
  };
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", onFocus);
  onFocus();
  createEffect(() => {
    if (paired()) live?.start();
    else live?.stop();
  });
  onCleanup(() => {
    clearInterval(watch);
    document.removeEventListener("visibilitychange", onVisible);
    window.removeEventListener("online", onOnline);
    window.removeEventListener("focus", onFocus);
    window.removeEventListener("blur", onFocus);
    live?.stop();
  });

  // the kept conversations, read each time the palette opens so a chat kept a moment ago can be found
  const [chats, setChats] = createSignal<KeptChat[]>([]);
  createEffect(() => {
    if (paletteOpen()) void api.keptChats().then((r) => setChats(r.sessions)).catch(() => setChats([]));
  });
  /** Opens a kept conversation in the chat: handed over the way the other screens hand the chat an attachment. */
  const openChat = (id: string) => {
    try { sessionStorage.setItem("kk-chat-open", id); } catch { /* the chat then opens on a new conversation */ }
    if (location.hash === "#/chat") window.dispatchEvent(new Event("kk:chat-open"));
    else location.hash = "#/chat";
  };
  const go = (hash: string) => () => {
    location.hash = hash;
  };
  // each kind of row is made in its own memo, so a list arriving (the kept conversations, read when the palette opens) makes
  // only its own rows again and the row under the cursor is not redrawn under it
  const screenRows = createMemo<PaletteItem[]>(() => {
    const screen = (id: string, label: string, run: () => void): PaletteItem => ({ id, label, group: t("palette.screens"), kind: t("palette.kind.screen"), run });
    // in the order of the menu at the side: its main places, then those under More, then the two at its foot
    const screens: PaletteItem[] = [
      screen("new", t("nav.new"), go("#/")),
      screen("inbox", t("nav.inbox"), go("#/inbox")),
      screen("chat", t("nav.chat"), go("#/chat")),
      screen("docs", t("nav.docs"), go("#/docs")),
      screen("projects", t("nav.projects"), go("#/projects")),
      screen("team", t("nav.team"), go("#/team")),
      screen("memory", t("nav.memory"), go("#/memory")),
      screen("meeting", t("nav.meeting"), go("#/meeting")),
      screen("explore", t("nav.explore"), go("#/explore")),
      screen("code", t("nav.code"), go("#/code")),
      screen("roundtable", t("nav.roundtable"), go("#/roundtable")),
      screen("handoff", t("nav.handoff"), go("#/handoff")),
      screen("instructions", t("nav.instructions"), go("#/instructions")),
      screen("skills", t("nav.skills"), go("#/skills")),
      screen("break", t("break.title"), go("#/break")),
      screen("agent", t("nav.agent"), go("#/agent")),
      screen("activity", t("nav.activity"), go("#/activity")),
      screen("verify", t("nav.verify"), () => { location.href = "/verify?from=workspace"; }),
      screen("privacy", t("nav.privacy"), go("#/privacy")),
      screen("settings", t("nav.settings"), go("#/settings")),
      { id: "shortcuts", label: t("kb.title"), group: t("palette.screens"), kind: t("palette.kind.action"), run: () => setHelpOpen(true) },
    ];
    return screens;
  });
  const reviewRows = createMemo<PaletteItem[]>(() =>
    jobs().map((j) => ({
      id: `job-${j.id}`,
      label: j.repo.split("/").pop() ?? j.repo,
      detail: t("nav.sub", { state: t(`state.${stateOf(j)}` as never), recipient: j.recipient }),
      keywords: `${j.id} ${j.repo}`,
      group: t("palette.reviews"),
      kind: t("palette.kind.review"),
      run: go(`#/job/${j.id}`),
    })),
  );
  const chatRows = createMemo<PaletteItem[]>(() =>
    chats().map((c) => ({
      id: `chat-${c.id}`,
      label: c.title || t("chat.untitled"),
      detail: t("chat.turns", { n: c.turns }),
      group: t("palette.chats"),
      kind: t("palette.kind.chat"),
      run: () => openChat(c.id),
    })),
  );
  const settingRows = createMemo<PaletteItem[]>(() => [
      { id: "theme-light", label: t("theme.light"), group: t("palette.theme"), kind: t("palette.kind.setting"), run: () => chooseTheme("light") },
      { id: "theme-dark", label: t("theme.dark"), group: t("palette.theme"), kind: t("palette.kind.setting"), run: () => chooseTheme("dark") },
      { id: "theme-system", label: t("theme.system"), group: t("palette.theme"), kind: t("palette.kind.setting"), run: () => chooseTheme("system") },
      { id: "signout", label: t("top.signout"), group: t("palette.account"), kind: t("palette.kind.action"), run: () => void api.logout().then(() => setPaired(false)) },
  ]);
  const paletteItems = createMemo<PaletteItem[]>(() => [...screenRows(), ...reviewRows(), ...chatRows(), ...settingRows()]);
  /** Conversations found by what was said in them, for words that are not in their names. */
  const searchSaid = async (q: string): Promise<PaletteItem[]> =>
    (await api.searchChats(q)).results.slice(0, 8).map((h) => ({
      // the same id as the conversation's row found by its name, so a conversation found both ways is listed once
      id: `chat-${h.id}`,
      label: h.title || t("chat.untitled"),
      detail: (h.snippets[0]?.text ?? "").replace(/\s+/g, " ").trim(),
      group: t("palette.chats"),
      kind: t("palette.kind.chat"),
      run: () => openChat(h.id),
    }));

  const jobId = () => /^#\/job\/([\w-]+)/.exec(hash())?.[1] ?? null;
  // screens of records (a review, the activity log, memory) take the wider measure of the one page grid; every other
  // screen is read or filled in, and keeps to the narrower one
  const pageKind = () => (jobId() !== null || hash() === "#/activity" || hash() === "#/memory" ? "data" : "read");

  return (
    <Show when={paired() !== null} fallback={<main class="center"><p class="muted">{t("app.connecting")}</p></main>}>
      <Show when={paired()} fallback={<Show when={unreachable()} fallback={<Pair onDone={() => { setPaired(null); }} />}><main class="center"><p role="alert" class="bad">{t("app.unreachable")}</p><button onClick={() => { setUnreachable(false); setPaired(null); }}>{t("app.retry")}</button></main></Show>}>
        <Opening />
        <Palette open={paletteOpen()} items={paletteItems()} search={searchSaid} onClose={() => setPaletteOpen(false)} />
        <ShortcutHelp open={helpOpen()} onClose={() => setHelpOpen(false)} />
        <div class="shell" inert={paletteOpen() || helpOpen()}>
          {/* the first stop of the Tab key: past the sidebar and its list of reviews, straight to the page */}
          <a class="skip-link" href="#main-content" onClick={(e) => { e.preventDefault(); const m = document.getElementById("main-content"); m?.focus(); m?.scrollIntoView({ block: "start" }); }}>{t("app.skip")}</a>
          <header class="top">
            <a class="brand" href="#/" aria-label="Kakurega"><span class="brand-mark" aria-hidden="true"><Glyph /></span><span class="brand-word">Kakurega</span></a>
            <Show when={config()}>{(c) => <Badge config={c()} modelDown={modelDown()} />}</Show>
            <button class="link commands" data-keys={/Mac|iPhone|iPad/.test(navigator.platform) ? "⌘K" : "Ctrl K"} onClick={() => setPaletteOpen(true)} aria-keyshortcuts="Meta+K Control+K Meta+Shift+P Control+Shift+P">{t("palette.open")}</button>
            <button class="link" onClick={async () => { await api.logout(); setPaired(false); }}>{t("top.signout")}</button>
          </header>
          <Show when={failures() >= 3}>
            <div class="banner" role="status">{t("app.connectionLost")}</div>
          </Show>
          <Show when={config()?.test_mode}>
            <div class="banner" role="status">{t("banner.test")}</div>
          </Show>
          <button type="button" class="menu-toggle" aria-expanded={menuOpen()} aria-controls="side-nav" onClick={() => setMenuOpen(!menuOpen())}>{t("nav.menu")}</button>
          <div class="body">
            <nav id="side-nav" class={menuOpen() ? "side open" : "side"} aria-label={t("nav.label")}>
              <a class="primary new-review" href="#/"><span>{t("nav.new")}</span></a>
              <a href="#/inbox" class={hash() === "#/inbox" ? "navlink active" : "navlink"}><NavIcon d="M2.5 9.5h3l1 2h3l1-2h3M2.5 9.5l1.6-5.2a1 1 0 0 1 1-.8h5.8a1 1 0 0 1 1 .8l1.6 5.2v3a1 1 0 0 1-1 1h-10a1 1 0 0 1-1-1z" />{t("nav.inbox")}{waiting() > 0 ? <span class="count" aria-label={t("nav.inboxCount", { n: waiting() })}>{waiting()}</span> : null}</a>
              <a href="#/chat" class={hash() === "#/chat" ? "navlink active" : "navlink"}><NavIcon d="M3 3.5h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H7l-3 2.5v-2.5H3a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1z" />{t("nav.chat")}</a>
              <a href="#/docs" class={hash() === "#/docs" ? "navlink active" : "navlink"}><NavIcon d="M4 2h5.5L12 4.5V14H4zM9.5 2v2.5H12M6 8h4M6 10.5h4" />{t("nav.docs")}</a>
              <a href="#/projects" class={hash() === "#/projects" ? "navlink active" : "navlink"}><NavIcon d="M2 4.5a1 1 0 0 1 1-1h3.5l1.5 1.5h5a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z" />{t("nav.projects")}</a>
              <a href="#/team" class={hash().startsWith("#/team") ? "navlink active" : "navlink"}><NavIcon d="M6 7.5a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM2.5 13c0-2 1.6-3.5 3.5-3.5S9.5 11 9.5 13M11 4a1.8 1.8 0 0 1 0 3.5M11.5 9.7c1.2.4 2 1.6 2 3.3" />{t("nav.team")}</a>
              <a href="#/memory" class={hash() === "#/memory" ? "navlink active" : "navlink"}><NavIcon d="M8 2.5v11M3 5.5a5 5 0 0 1 10 0v5a5 5 0 0 1-10 0z" />{t("nav.memory")}</a>
              {/* everything else stays one click away; the group opens by itself when one of its screens is shown */}
              <details id="nav-more" class="nav-more" open={moreOpen() ?? inMore()} onToggle={(e) => { if (e.currentTarget.open !== (moreOpen() ?? inMore())) setMoreOpen(e.currentTarget.open); }}>
                <summary>{t("nav.more")}</summary>
                <h2>{t("nav.group.work")}</h2>
                <a href="#/meeting" class={hash() === "#/meeting" ? "navlink active" : "navlink"}><NavIcon d="M3 3.5h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H7l-3 2.5v-2.5H3a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1zM5 7h6M5 9h4" />{t("nav.meeting")}</a>
                <a href="#/explore" class={hash() === "#/explore" ? "navlink active" : "navlink"}><NavIcon d="M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zM10.6 10.6 14 14" />{t("nav.explore")}</a>
                <a href="#/code" class={hash() === "#/code" ? "navlink active" : "navlink"}><NavIcon d="M5.5 4.5 2 8l3.5 3.5M10.5 4.5 14 8l-3.5 3.5M9 3 7 13" />{t("nav.code")}</a>
                <a href="#/roundtable" class={hash() === "#/roundtable" ? "navlink active" : "navlink"}><NavIcon d="M8 3a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM3.5 9.5a1.8 1.8 0 1 0 0 3.6 1.8 1.8 0 0 0 0-3.6zM12.5 9.5a1.8 1.8 0 1 0 0 3.6 1.8 1.8 0 0 0 0-3.6z" />{t("nav.roundtable")}</a>
                <a href="#/handoff" class={hash() === "#/handoff" ? "navlink active" : "navlink"}><NavIcon d="M9 3h4v4M13 3 7.5 8.5M11 9.5V13H3V5h3.5" />{t("nav.handoff")}</a>
                <h2>{t("nav.group.knows")}</h2>
                <a href="#/instructions" class={hash() === "#/instructions" ? "navlink active" : "navlink"}><NavIcon d="M3 4h10M3 8h10M3 12h6" />{t("nav.instructions")}</a>
                <a href="#/skills" class={hash() === "#/skills" ? "navlink active" : "navlink"}><NavIcon d="M8 2l1.6 4.4L14 8l-4.4 1.6L8 14l-1.6-4.4L2 8l4.4-1.6z" />{t("nav.skills")}</a>
                <h2>{t("nav.group.trust")}</h2>
                <a href="#/break" class={hash() === "#/break" ? "navlink active" : "navlink"}><NavIcon d="M8 2l5 2v4c0 3-2.2 5-5 6-2.8-1-5-3-5-6V4z" />{t("break.title")}</a>
                <a href="#/agent" class={hash() === "#/agent" ? "navlink active" : "navlink"}><NavIcon d="M6 2v3M10 2v3M4 5h8v3a4 4 0 0 1-8 0zM8 12v2" />{t("nav.agent")}</a>
                <a href="#/activity" class={hash() === "#/activity" ? "navlink active" : "navlink"}><NavIcon d="M2 8h3l2-4 3 8 2-4h2" />{t("nav.activity")}</a>
                <a href="/verify?from=workspace" class="navlink"><NavIcon d="M8 14A6 6 0 1 0 8 2a6 6 0 0 0 0 12zM5.5 8l2 2 3.5-4" />{t("nav.verify")}</a>
              </details>
              <label class="sr" for="search">{t("nav.search.label")}</label>
              <input
                id="search"
                type="search"
                placeholder={t("nav.search.placeholder")}
                value={query()}
                onInput={(e) => setQuery(e.currentTarget.value)}
                onKeyDown={(e) => { if (e.key === "Escape") { setQuery(""); e.currentTarget.blur(); } }}
              />
              <h2>{t("nav.heading")}</h2>
              <ul class="list">
                <For each={shown()} fallback={<li class="muted">{jobs().length === 0 ? t("nav.none") : t("nav.nomatch")}</li>}>
                  {(j) => (
                    <li>
                      <a href={`#/job/${j.id}`} class={jobId() === j.id ? "active" : ""}>
                        <span class={`state ${j.state}`} aria-hidden="true" />
                        <span class="name">{j.repo.split("/").pop()}</span>
                        <span class="sub">{t("nav.sub", { state: t(`state.${stateOf(j)}` as never), recipient: j.recipient })}</span>
                      </a>
                    </li>
                  )}
                </For>
              </ul>
              <div class="side-foot">
                <a href="#/privacy" class={hash() === "#/privacy" ? "navlink active" : "navlink"}><NavIcon d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8zM8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4z" />{t("nav.privacy")}</a>
                <a href="#/settings" class={hash() === "#/settings" ? "navlink active" : "navlink"}><NavIcon d="M3 5h6M12 5h1M3 11h1M7 11h6M10.5 3.5v3M5.5 9.5v3" />{t("nav.settings")}</a>
                <Show when={config()}>{(c) => <Safeguards config={c()} />}</Show>
                <div class="side-prefs"><LanguagePicker /><ThemePicker /></div>
              </div>
            </nav>
            <main class="main" id="main-content" tabindex="-1" data-page={pageKind()}>
              <Show when={welcome()}>
                <Welcome onDone={() => setWelcome(false)} />
              </Show>
              <ErrorBoundary fallback={(_err, reset) => <section class="card stack" role="alert"><p>{t("app.crashed")}</p><button onClick={reset}>{t("app.reload")}</button></section>}>
              <Show when={config()}>
                {(c) => (
                  <Show when={hash() !== "#/memory" && hash() !== "#/instructions" && hash() !== "#/settings" && hash() !== "#/activity" && hash() !== "#/chat" && hash() !== "#/roundtable" && !hash().startsWith("#/team") && hash() !== "#/docs" && hash() !== "#/meeting" && hash() !== "#/skills" && hash() !== "#/projects" && hash() !== "#/handoff" && hash() !== "#/break" && hash() !== "#/code" && hash() !== "#/explore" && hash() !== "#/inbox" && hash() !== "#/agent" && hash() !== "#/privacy"} fallback={hash() === "#/settings" ? <Settings /> : hash() === "#/instructions" ? <Instructions /> : hash() === "#/privacy" ? <Privacy /> : hash() === "#/agent" ? <AgentAccess /> : hash() === "#/inbox" ? <Inbox /> : hash() === "#/activity" ? <Activity /> : hash() === "#/chat" ? <Chat /> : hash() === "#/roundtable" ? <Roundtable /> : hash().startsWith("#/team") ? <Team id={hash().slice(7)} /> : hash() === "#/docs" ? <Documents /> : hash() === "#/meeting" ? <Meeting /> : hash() === "#/skills" ? <Skills /> : hash() === "#/projects" ? <Projects /> : hash() === "#/handoff" ? <Handoff /> : hash() === "#/break" ? <BreakIt /> : hash() === "#/code" ? <CodeTask /> : hash() === "#/explore" ? <Explore /> : <Memory />}>
                    <Show when={jobId()} fallback={<NewReview config={c()} onStarted={(id) => { void refresh(); location.hash = `#/job/${id}`; }} />}>
                    {(id) => <ReviewView id={id()} config={c()} onUpdate={() => void refresh()} />}
                    </Show>
                  </Show>
                )}
              </Show>
              </ErrorBoundary>
            </main>
          </div>
        </div>
      </Show>
    </Show>
  );
}
