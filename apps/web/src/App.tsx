import { createEffect, createMemo, createSignal, ErrorBoundary, For, lazy, onCleanup, onMount, Show, Suspense } from "solid-js";
import { api, ApiError, setCsrf, type Config, type Job } from "./api";
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
  const fromLink = /#pair=([A-Za-z0-9_-]+)/.exec(location.hash)?.[1];
  if (fromLink) queueMicrotask(() => submit(fromLink));
  return (
    <main class="center">
      <h1>{t("pair.title")}</h1>
      <p class="muted">{rich("pair.intro")}</p>
      <form
        class="stack"
        onSubmit={(e) => {
          e.preventDefault();
          submit(code());
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

function Badge(props: { config: Config; modelDown: boolean }) {
  const r = () => props.config.route;
  return (
    <div class="badges">
      <Show
        when={r().stays_here !== false || r().simulated}
        fallback={
          <span class="badge warn" title={t("badge.remote.title", { host: r().host ?? "" })}>
            {r().host ? t("badge.remote", { host: r().host ?? "" }) : t("badge.unknown")}
          </span>
        }
      >
        <span class="badge ok" title={t("badge.private.title")}>
          <span class="dot" /> {t("badge.private")}
        </span>
      </Show>
      <span class="badge" title={`${t("badge.sandbox.title")} (${r().transport ?? ""})`}>
        {sandboxWords(r().transport)}
      </span>
      <Show when={props.config.approvals?.asks_every_release}>
        <span class="badge" title={t(props.config.approvals?.signing_key_enrolled ? "badge.approvals.signed.title" : "badge.approvals.title")}>
          {t(props.config.approvals?.signing_key_enrolled ? "badge.approvals.signed" : "badge.approvals")}
        </span>
      </Show>
      <span class={props.modelDown ? "badge warn" : "badge"} title={props.modelDown ? t("badge.model.down.title") : undefined}>
        {r().model === "scripted test model" ? t("badge.model.scripted") : (r().model ?? r().id)}
        <Show when={props.modelDown}> · {t("badge.model.down")}</Show>
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
  let searchBox: HTMLInputElement | undefined;
  // on a narrow screen the menu is closed until asked for; choosing a page closes it again
  const [menuOpen, setMenuOpen] = createSignal(false);
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
    return jobs().filter((j) => `${j.repo} ${j.recipient} ${j.state.replace(/_/g, " ")}`.toLowerCase().includes(q));
  });
  const onKey = (e: KeyboardEvent) => {
    if (e.isComposing || e.defaultPrevented) return;
    const found = appShortcut(e, e.target as HTMLElement | null, singleKeys());
    if (!found) return;
    e.preventDefault();
    if (found.id === "palette") {
      if (paired()) setPaletteOpen(!paletteOpen());
    } else if (found.id === "search") {
      setMenuOpen(true);
      searchBox?.focus();
      searchBox?.select();
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
  const [modelDown, setModelDown] = createSignal(false);
  let modelChecked = 0;
  const checkModel = async (force = false) => {
    if (!paired() || (!force && Date.now() - modelChecked < 60_000)) return;
    modelChecked = Date.now();
    try {
      const r = await api.readiness();
      setModelDown(r.checks.some((c) => c.id === "ollama" && !c.ok));
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

  const paletteItems = createMemo<PaletteItem[]>(() => {
    const go = (hash: string) => () => {
      location.hash = hash;
    };
    const screens: PaletteItem[] = [
      { id: "new", label: t("nav.new"), group: t("palette.screens"), run: go("#/") },
      { id: "inbox", label: t("nav.inbox"), group: t("palette.screens"), run: go("#/inbox") },
      { id: "docs", label: t("nav.docs"), group: t("palette.screens"), run: go("#/docs") },
      { id: "meeting", label: t("nav.meeting"), group: t("palette.screens"), run: go("#/meeting") },
      { id: "break", label: t("break.title"), group: t("palette.screens"), run: go("#/break") },
      { id: "skills", label: t("nav.skills"), group: t("palette.screens"), run: go("#/skills") },
      { id: "projects", label: t("nav.projects"), group: t("palette.screens"), run: go("#/projects") },
      { id: "handoff", label: t("nav.handoff"), group: t("palette.screens"), run: go("#/handoff") },
      { id: "code", label: t("nav.code"), group: t("palette.screens"), run: go("#/code") },
      { id: "explore", label: t("nav.explore"), group: t("palette.screens"), run: go("#/explore") },
      { id: "chat", label: t("nav.chat"), group: t("palette.screens"), run: go("#/chat") },
      { id: "roundtable", label: t("nav.roundtable"), group: t("palette.screens"), run: go("#/roundtable") },
      { id: "team", label: t("nav.team"), group: t("palette.screens"), run: go("#/team") },
      { id: "memory", label: t("nav.memory"), group: t("palette.screens"), run: go("#/memory") },
      { id: "instructions", label: t("nav.instructions"), group: t("palette.screens"), run: go("#/instructions") },
      { id: "settings", label: t("nav.settings"), group: t("palette.screens"), run: go("#/settings") },
      { id: "privacy", label: t("nav.privacy"), group: t("palette.screens"), run: go("#/privacy") },
      { id: "agent", label: t("nav.agent"), group: t("palette.screens"), run: go("#/agent") },
      { id: "activity", label: t("nav.activity"), group: t("palette.screens"), run: go("#/activity") },
      { id: "shortcuts", label: t("kb.title"), group: t("palette.screens"), run: () => setHelpOpen(true) },
    ];
    const reviews: PaletteItem[] = jobs().map((j) => ({ id: `job-${j.id}`, label: t("nav.sub", { state: t(`state.${j.state}` as never), recipient: j.recipient }) + ` · ${j.id}`, group: t("palette.reviews"), run: go(`#/job/${j.id}`) }));
    const settings: PaletteItem[] = [
      { id: "theme-light", label: t("theme.light"), group: t("palette.theme"), run: () => chooseTheme("light") },
      { id: "theme-dark", label: t("theme.dark"), group: t("palette.theme"), run: () => chooseTheme("dark") },
      { id: "theme-system", label: t("theme.system"), group: t("palette.theme"), run: () => chooseTheme("system") },
      { id: "signout", label: t("top.signout"), group: t("palette.account"), run: () => void api.logout().then(() => setPaired(false)) },
    ];
    return [...screens, ...reviews, ...settings];
  });

  const jobId = () => /^#\/job\/([\w-]+)/.exec(hash())?.[1] ?? null;

  return (
    <Show when={paired() !== null} fallback={<main class="center"><p class="muted">{t("app.connecting")}</p></main>}>
      <Show when={paired()} fallback={<Show when={unreachable()} fallback={<Pair onDone={() => { setPaired(null); }} />}><main class="center"><p role="alert" class="bad">{t("app.unreachable")}</p><button onClick={() => { setUnreachable(false); setPaired(null); }}>{t("app.retry")}</button></main></Show>}>
        <Palette open={paletteOpen()} items={paletteItems()} onClose={() => setPaletteOpen(false)} />
        <ShortcutHelp open={helpOpen()} onClose={() => setHelpOpen(false)} />
        <div class="shell" inert={paletteOpen() || helpOpen()}>
          <header class="top">
            <a class="brand" href="#/">kakurega</a>
            <Show when={config()}>{(c) => <Badge config={c()} modelDown={modelDown()} />}</Show>
            <LanguagePicker />
            <ThemePicker />
            <button class="link" onClick={() => setPaletteOpen(true)} aria-keyshortcuts="Control+Shift+P Meta+Shift+P">{t("palette.open")}</button>
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
              <a class="primary" href="#/">{t("nav.new")}</a>
              <a href="#/inbox" class={hash() === "#/inbox" ? "navlink active" : "navlink"}>{t("nav.inbox")}{waiting() > 0 ? <span class="count" aria-label={t("nav.inboxCount", { n: waiting() })}> ({waiting()})</span> : null}</a>
              <a href="#/privacy" class={hash() === "#/privacy" ? "navlink active" : "navlink"}>{t("nav.privacy")}</a>
              <h2>{t("nav.group.work")}</h2>
              <a href="#/projects" class={hash() === "#/projects" ? "navlink active" : "navlink"}>{t("nav.projects")}</a>
              <a href="#/chat" class={hash() === "#/chat" ? "navlink active" : "navlink"}>{t("nav.chat")}</a>
              <a href="#/docs" class={hash() === "#/docs" ? "navlink active" : "navlink"}>{t("nav.docs")}</a>
              <a href="#/meeting" class={hash() === "#/meeting" ? "navlink active" : "navlink"}>{t("nav.meeting")}</a>
              <a href="#/explore" class={hash() === "#/explore" ? "navlink active" : "navlink"}>{t("nav.explore")}</a>
              <a href="#/code" class={hash() === "#/code" ? "navlink active" : "navlink"}>{t("nav.code")}</a>
              <a href="#/roundtable" class={hash() === "#/roundtable" ? "navlink active" : "navlink"}>{t("nav.roundtable")}</a>
              <a href="#/team" class={hash().startsWith("#/team") ? "navlink active" : "navlink"}>{t("nav.team")}</a>
              <a href="#/handoff" class={hash() === "#/handoff" ? "navlink active" : "navlink"}>{t("nav.handoff")}</a>
              <h2>{t("nav.group.knows")}</h2>
              <a href="#/memory" class={hash() === "#/memory" ? "navlink active" : "navlink"}>{t("nav.memory")}</a>
              <a href="#/instructions" class={hash() === "#/instructions" ? "navlink active" : "navlink"}>{t("nav.instructions")}</a>
              <a href="#/skills" class={hash() === "#/skills" ? "navlink active" : "navlink"}>{t("nav.skills")}</a>
              <h2>{t("nav.group.trust")}</h2>
              <a href="#/break" class={hash() === "#/break" ? "navlink active" : "navlink"}>{t("break.title")}</a>
              <a href="#/agent" class={hash() === "#/agent" ? "navlink active" : "navlink"}>{t("nav.agent")}</a>
              <a href="#/activity" class={hash() === "#/activity" ? "navlink active" : "navlink"}>{t("nav.activity")}</a>
              <a href="#/settings" class={hash() === "#/settings" ? "navlink active" : "navlink"}>{t("nav.settings")}</a>
              <label class="sr" for="search">{t("nav.search.label")}</label>
              <input
                id="search"
                type="search"
                placeholder={t("nav.search.placeholder")}
                ref={searchBox}
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
                        <span class="sub">{t("nav.sub", { state: t(`state.${j.state}` as never), recipient: j.recipient })}</span>
                      </a>
                    </li>
                  )}
                </For>
              </ul>
            </nav>
            <main class="main">
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
