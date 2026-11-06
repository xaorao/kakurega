import { createEffect, createMemo, createSignal, ErrorBoundary, For, onCleanup, onMount, Show } from "solid-js";
import { api, ApiError, setCsrf, type Config, type Job } from "./api";
void ApiError;
import { NewReview } from "./screens/NewReview";
import { ReviewView } from "./screens/ReviewView";
import { Verify } from "./screens/Verify";
import { Memory } from "./screens/Memory";
import { Instructions } from "./screens/Instructions";
import { Activity } from "./screens/Activity";
import { AgentAccess } from "./screens/AgentAccess";
import { Privacy } from "./screens/Privacy";
import { Chat } from "./screens/Chat";
import { Documents } from "./screens/Documents";
import { Inbox } from "./screens/Inbox";
import { notifier } from "./notifier";
import { Palette, type PaletteItem } from "./components/Palette";
import { ShortcutHelp } from "./components/ShortcutHelp";
import { appShortcut } from "./shortcuts";
import { createLive } from "./live";
import { clearDrafts } from "./composer";
import { applyLocale, LOCALES, locale, rich, setLocale, t, type Locale } from "./i18n";

function useHash() {
  const [hash, setHash] = createSignal(location.hash);
  const on = () => setHash(location.hash);
  window.addEventListener("hashchange", on);
  onCleanup(() => window.removeEventListener("hashchange", on));
  return hash;
}

type Theme = "system" | "light" | "dark";

function applyTheme(t: Theme) {
  if (t === "system") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", t);
}

function storedTheme(): Theme {
  try {
    const v = localStorage.getItem("kk-theme");
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

const [theme, setTheme] = createSignal<Theme>(storedTheme());

function chooseTheme(t: Theme) {
  setTheme(t);
  applyTheme(t);
  try {
    if (t === "system") localStorage.removeItem("kk-theme");
    else localStorage.setItem("kk-theme", t);
  } catch {
    /* the choice still applies for this visit */
  }
}

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

function Badge(props: { config: Config }) {
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
      <span class="badge" title={t("badge.sandbox.title")}>
        {r().transport ?? "sandbox"}
      </span>
      <span class="badge">{r().model ?? r().id}</span>
    </div>
  );
}

export function App() {
  if (location.pathname === "/verify") return <Verify />;
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
  let searchBox: HTMLInputElement | undefined;
  const shown = createMemo(() => {
    const q = query().trim().toLowerCase();
    if (!q) return jobs();
    return jobs().filter((j) => `${j.repo} ${j.recipient} ${j.state.replace(/_/g, " ")}`.toLowerCase().includes(q));
  });
  const onKey = (e: KeyboardEvent) => {
    if (e.isComposing || e.defaultPrevented) return;
    const found = appShortcut(e, e.target as HTMLElement | null);
    if (!found) return;
    e.preventDefault();
    if (found.id === "palette") {
      if (paired()) setPaletteOpen(!paletteOpen());
    } else if (found.id === "search") {
      searchBox?.focus();
      searchBox?.select();
    } else if (found.id === "help" || found.id === "help-anywhere") {
      if (paired() && !paletteOpen()) setHelpOpen(!helpOpen());
    }
  };
  applyLocale();
  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));

  const load = async () => {
    try {
      const s = await api.session();
      if (!s.paired) {
        setPaired(false);
        return;
      }
      setCsrf(s.csrf);
      const [c, j] = await Promise.all([api.config(), api.jobs()]);
      setConfig(c);
      setJobs(j.jobs);
      setPaired(true);
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
  const onFocus = () => void notifier.focusChanged();
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
      { id: "chat", label: t("nav.chat"), group: t("palette.screens"), run: go("#/chat") },
      { id: "memory", label: t("nav.memory"), group: t("palette.screens"), run: go("#/memory") },
      { id: "instructions", label: t("nav.instructions"), group: t("palette.screens"), run: go("#/instructions") },
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
            <Show when={config()}>{(c) => <Badge config={c()} />}</Show>
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
          <div class="body">
            <nav class="side" aria-label={t("nav.label")}>
              <a class="primary" href="#/">{t("nav.new")}</a>
              <a href="#/inbox" class={hash() === "#/inbox" ? "navlink active" : "navlink"}>{t("nav.inbox")}{waiting() > 0 ? <span class="count" aria-label={t("nav.inboxCount", { n: waiting() })}> ({waiting()})</span> : null}</a>
              <a href="#/docs" class={hash() === "#/docs" ? "navlink active" : "navlink"}>{t("nav.docs")}</a>
              <a href="#/chat" class={hash() === "#/chat" ? "navlink active" : "navlink"}>{t("nav.chat")}</a>
              <a href="#/memory" class={hash() === "#/memory" ? "navlink active" : "navlink"}>{t("nav.memory")}</a>
              <a href="#/instructions" class={hash() === "#/instructions" ? "navlink active" : "navlink"}>{t("nav.instructions")}</a>
              <a href="#/privacy" class={hash() === "#/privacy" ? "navlink active" : "navlink"}>{t("nav.privacy")}</a>
              <a href="#/agent" class={hash() === "#/agent" ? "navlink active" : "navlink"}>{t("nav.agent")}</a>
              <a href="#/activity" class={hash() === "#/activity" ? "navlink active" : "navlink"}>{t("nav.activity")}</a>
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
              <ErrorBoundary fallback={(_err, reset) => <section class="card stack" role="alert"><p>{t("app.crashed")}</p><button onClick={reset}>{t("app.reload")}</button></section>}>
              <Show when={config()}>
                {(c) => (
                  <Show when={hash() !== "#/memory" && hash() !== "#/instructions" && hash() !== "#/activity" && hash() !== "#/chat" && hash() !== "#/docs" && hash() !== "#/inbox" && hash() !== "#/agent" && hash() !== "#/privacy"} fallback={hash() === "#/instructions" ? <Instructions /> : hash() === "#/privacy" ? <Privacy /> : hash() === "#/agent" ? <AgentAccess /> : hash() === "#/inbox" ? <Inbox /> : hash() === "#/activity" ? <Activity /> : hash() === "#/chat" ? <Chat /> : hash() === "#/docs" ? <Documents /> : <Memory />}>
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
