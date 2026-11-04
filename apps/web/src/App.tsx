import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { api, ApiError, setCsrf, type Config, type Job } from "./api";
void ApiError;
import { NewReview } from "./screens/NewReview";
import { ReviewView } from "./screens/ReviewView";
import { Verify } from "./screens/Verify";

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

function ThemePicker() {
  const [theme, setTheme] = createSignal<Theme>(storedTheme());
  onMount(() => applyTheme(theme()));
  const choose = (t: Theme) => {
    setTheme(t);
    applyTheme(t);
    try {
      if (t === "system") localStorage.removeItem("kk-theme");
      else localStorage.setItem("kk-theme", t);
    } catch {
      /* the choice still applies for this visit */
    }
  };
  return (
    <label class="theme">
      <span class="sr">Theme</span>
      <select value={theme()} onChange={(e) => choose(e.currentTarget.value as Theme)}>
        <option value="system">Match system</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
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
      setError(e instanceof ApiError ? e.message : "Could not reach the local service.");
    } finally {
      setBusy(false);
    }
  };
  const fromLink = /#pair=([A-Za-z0-9_-]+)/.exec(location.hash)?.[1];
  if (fromLink) queueMicrotask(() => submit(fromLink));
  return (
    <main class="center">
      <h1>Pair this browser</h1>
      <p class="muted">Run <code>kaku pair</code> in a terminal on this computer and open the link it prints, or paste the code here.</p>
      <form
        class="stack"
        onSubmit={(e) => {
          e.preventDefault();
          submit(code());
        }}
      >
        <label for="code">One-time code</label>
        <input id="code" autocomplete="off" spellcheck={false} value={code()} onInput={(e) => setCode(e.currentTarget.value)} />
        <button type="submit" disabled={busy() || code().trim().length === 0}>Pair</button>
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
      <span class="badge ok" title="The model runs on this computer. Your files are not sent anywhere.">
        <span class="dot" /> Private: model runs on this computer
      </span>
      <span class="badge" title="The agent runs in a sealed sandbox with no network.">
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
  const [jobs, setJobs] = createSignal<Job[]>([]);
  const [query, setQuery] = createSignal("");
  let searchBox: HTMLInputElement | undefined;
  const shown = createMemo(() => {
    const q = query().trim().toLowerCase();
    if (!q) return jobs();
    return jobs().filter((j) => `${j.repo} ${j.recipient} ${j.state.replace(/_/g, " ")}`.toLowerCase().includes(q));
  });
  const onKey = (e: KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      searchBox?.focus();
      searchBox?.select();
    }
  };
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
    } catch {
      setPaired(false);
    }
  };
  createEffect(() => {
    if (paired() === null) void load();
  });
  const refresh = async () => {
    try {
      setJobs((await api.jobs()).jobs);
    } catch {
      /* the next poll will try again */
    }
  };
  const timer = setInterval(() => paired() && void refresh(), 1500);
  onCleanup(() => clearInterval(timer));

  const jobId = () => /^#\/job\/([\w-]+)/.exec(hash())?.[1] ?? null;

  return (
    <Show when={paired() !== null} fallback={<main class="center"><p class="muted">Connecting…</p></main>}>
      <Show when={paired()} fallback={<Pair onDone={() => { setPaired(null); }} />}>
        <div class="shell">
          <header class="top">
            <a class="brand" href="#/">kakurega</a>
            <Show when={config()}>{(c) => <Badge config={c()} />}</Show>
            <ThemePicker />
            <button class="link" onClick={async () => { await api.logout(); setPaired(false); }}>Sign out</button>
          </header>
          <Show when={config()?.test_mode}>
            <div class="banner" role="status">TEST MODE: the model is scripted. Results below do not show what a real model finds.</div>
          </Show>
          <div class="body">
            <nav class="side" aria-label="Reviews">
              <a class="primary" href="#/">New review</a>
              <label class="sr" for="search">Search reviews</label>
              <input
                id="search"
                type="search"
                placeholder="Search reviews (Cmd+K)"
                ref={searchBox}
                value={query()}
                onInput={(e) => setQuery(e.currentTarget.value)}
                onKeyDown={(e) => { if (e.key === "Escape") { setQuery(""); e.currentTarget.blur(); } }}
              />
              <h2>Reviews</h2>
              <ul class="list">
                <For each={shown()} fallback={<li class="muted">{jobs().length === 0 ? "None yet." : "No review matches."}</li>}>
                  {(j) => (
                    <li>
                      <a href={`#/job/${j.id}`} class={jobId() === j.id ? "active" : ""}>
                        <span class={`state ${j.state}`} aria-hidden="true" />
                        <span class="name">{j.repo.split("/").pop()}</span>
                        <span class="sub">{j.state.replace(/_/g, " ")} · for {j.recipient}</span>
                      </a>
                    </li>
                  )}
                </For>
              </ul>
            </nav>
            <main class="main">
              <Show when={config()}>
                {(c) => (
                  <Show when={jobId()} fallback={<NewReview config={c()} onStarted={(id) => { void refresh(); location.hash = `#/job/${id}`; }} />}>
                    {(id) => <ReviewView id={id()} config={c()} onUpdate={() => void refresh()} />}
                  </Show>
                )}
              </Show>
            </main>
          </div>
        </div>
      </Show>
    </Show>
  );
}
