import { Lead } from "../components/Lead";
import { createMemo, createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api, ApiError, type NearModel } from "../api";
import { LOCALES, locale, setLocale, t, type Key, type Locale } from "../i18n";
import { notifier, notifyState, turnNotifyOn } from "../notifier";
import { SECTIONS, SETTINGS, searchSettings, type Setting } from "../settings-catalog";
import { chooseTheme, theme, type Theme } from "../theme";
import { chooseSingleKeys, singleKeys } from "../single-keys";

/** The names that are swapped for placeholders before a question goes to a model that is not on this computer. */
function HiddenNames() {
  const [list, { refetch }] = createResource(() => api.hiddenNames());
  // whether the list is in use depends on where the model runs, which the owner can change further up this page
  const stale = () => void refetch();
  window.addEventListener("kk-config-stale", stale);
  onCleanup(() => window.removeEventListener("kk-config-stale", stale));
  const [name, setName] = createSignal("");
  const [error, setError] = createSignal("");
  const [done, setDone] = createSignal("");
  const edit = async (fn: () => Promise<{ changed: boolean }>, said: string) => {
    setError("");
    setDone("");
    try {
      const r = await fn();
      setName("");
      // said once the list shows it, so the words never sit beside a list that still has the old entry
      await refetch();
      setDone(r.changed ? said : t("set.pii.same"));
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("set.hide.error"));
    }
  };
  return (
    <div class="stack">
      <Show when={list()}>{(l) => <p class="muted" id="hide-applies">{l().applies ? t("set.hide.on") : t("set.hide.off")}</p>}</Show>
      <form class="actions" onSubmit={(e) => { e.preventDefault(); void edit(() => api.hiddenNameAdd(name()), t("set.hide.added")); }}>
        <label class="sr" for="hide-name">{t("set.hide.entry")}</label>
        <input id="hide-name" value={name()} maxlength="80" placeholder={t("set.hide.hint")} onInput={(e) => setName(e.currentTarget.value)} />
        <button class="primary" type="submit" disabled={name().trim().length < 2}>{t("set.hide.add")}</button>
      </form>
      <Show when={done()}><p role="status" class="ok">{done()}</p></Show>
      <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
      <Show when={list()} fallback={<p class="muted">{list.error ? t("set.hide.error") : t("review.loading")}</p>}>
        {(l) => (
          <ul class="plain stack" aria-label={t("set.hide.entries")}>
            <For each={l().names} fallback={<li class="muted">{t("set.hide.none")}</li>}>
              {(n) => (
                <li class="actions">
                  <span>{n.name}</span>
                  <button type="button" onClick={() => void edit(() => api.hiddenNameRemove(n.name), t("set.hide.removed"))}>{t("set.pii.remove")} <span class="sr">{n.name}</span></button>
                </li>
              )}
            </For>
          </ul>
        )}
      </Show>
    </div>
  );
}

/** The values the owner said are not personal data. They change only the release notice, which counts what they leave out. */
function PiiAllow() {
  const [list, { refetch }] = createResource(() => api.piiAllow());
  const [entry, setEntry] = createSignal("");
  const [error, setError] = createSignal("");
  const [done, setDone] = createSignal("");
  const edit = async (fn: () => Promise<{ changed: boolean }>, said: string) => {
    setError("");
    setDone("");
    try {
      const r = await fn();
      setEntry("");
      // said once the list shows it, so the words never sit beside a list that still has the old entry
      await refetch();
      setDone(r.changed ? said : t("set.pii.same"));
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("set.pii.error"));
    }
  };
  return (
    <div class="stack">
      <form class="actions" onSubmit={(e) => { e.preventDefault(); void edit(() => api.piiAllowAdd(entry()), t("set.pii.added")); }}>
        <label class="sr" for="pii-entry">{t("set.pii.entry")}</label>
        <input id="pii-entry" value={entry()} maxlength="200" placeholder={t("set.pii.hint")} onInput={(e) => setEntry(e.currentTarget.value)} />
        <button class="primary" type="submit" disabled={entry().trim().length === 0}>{t("set.pii.add")}</button>
      </form>
      <Show when={done()}><p role="status" class="ok">{done()}</p></Show>
      <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
      <Show when={list()} fallback={<p class="muted">{list.error ? t("set.pii.error") : t("review.loading")}</p>}>
        {(l) => (
          <ul class="plain stack" aria-label={t("set.pii.entries")}>
            <For each={l().entries} fallback={<li class="muted">{t("set.pii.none")}</li>}>
              {(e) => (
                <li class="actions">
                  <code>{e.entry}</code>
                  <button type="button" onClick={() => void edit(() => api.piiAllowRemove(e.entry), t("set.pii.removed"))}>{t("set.pii.remove")} <span class="sr">{e.entry}</span></button>
                </li>
              )}
            </For>
          </ul>
        )}
      </Show>
    </div>
  );
}

/** Where the model that answers runs: on this computer, or on NEAR AI Cloud's verified hardware with the owner's own key. */
function RouteChoice() {
  const [opts, { mutate, refetch }] = createResource(() => api.routeOptions());
  const [model, setModel] = createSignal("");
  const [key, setKey] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  // NEAR's own list of models, read only when the owner opens it
  const [listOpen, setListOpen] = createSignal(false);
  const [list] = createResource(listOpen, (open) => (open ? api.nearModels().then((l) => ({ ok: true as const, ...l })).catch((e) => ({ ok: false as const, why: e instanceof ApiError ? e.message : t("set.models.error") })) : null));
  const [find, setFind] = createSignal("");
  const shownModels = (all: NearModel[]) => { const q = find().trim().toLowerCase(); return q ? all.filter((m) => m.model.toLowerCase().includes(q)) : all; };
  // two decimals for ordinary prices, two significant digits for the very small ones
  const price = (n: number) => (n >= 0.095 ? n.toFixed(2) : String(Number(n.toPrecision(2))));
  const windowText = (n: number | null) => (!n ? "?" : n >= 1_000_000 ? `${Number((n / 1_000_000).toFixed(1))}M` : n >= 1000 ? `${Math.round(n / 1000)}K` : String(n));
  const [done, setDone] = createSignal("");
  const change = async (body: Parameters<typeof api.routeChoose>[0], said: string) => {
    setBusy(true);
    setError("");
    setDone("");
    try {
      mutate(await api.routeChoose(body));
      setKey("");
      setDone(said);
      window.dispatchEvent(new Event("kk-config-stale"));
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("set.route.error"));
      await refetch();
    } finally {
      setBusy(false);
    }
  };
  return (
    <div class="stack" id="route-choice">
      <Show when={opts()} fallback={<p class="muted">{opts.error ? t("set.route.error") : t("review.loading")}</p>}>
        {(o) => (
          <>
            <p id="route-current"><strong>{t(`set.route.now.${o().current ?? "local"}`, { model: (o().current === "near" ? o().near?.model : o().local?.model) ?? "", host: o().route.host ?? "" })}</strong></p>
            <Show when={o().changeable} fallback={<p class="muted">{t("set.route.fixed")}</p>}>
              <Show when={o().current !== "local"}>
                <div class="actions">
                  <button type="button" disabled={busy()} onClick={() => void change({ route: "local" }, t("set.route.done.local"))}>{t("set.route.useLocal")}</button>
                </div>
              </Show>
              <form class="stack" onSubmit={(e) => { e.preventDefault(); void change({ route: "near", model: (model().trim() || o().near?.model) ?? "", ...(key().trim() ? { key: key().trim() } : {}) }, t("set.route.done.near")); }}>
                <label for="route-model">{t("set.route.model")}</label>
                <input id="route-model" value={model() || (o().near?.model ?? "")} maxlength="200" placeholder={t("set.route.modelHint")} autocomplete="off" spellcheck={false} onInput={(e) => setModel(e.currentTarget.value)} />
                <details class="models-fold" onToggle={(e) => setListOpen(e.currentTarget.open)}>
                  <summary>{t("set.models.open")}</summary>
                  <Show when={list()} fallback={<p class="muted">{t("review.loading")}</p>}>
                    {(l) => (
                      <Show when={l().ok ? (l() as { models: NearModel[]; source: string }) : null} fallback={<p role="alert" class="bad">{(l() as { why: string }).why}</p>}>
                        {(ok) => (
                          <div class="stack">
                            <label for="models-find" class="visually-hidden">{t("set.models.find")}</label>
                            <input id="models-find" type="search" placeholder={t("set.models.find")} value={find()} onInput={(e) => setFind(e.currentTarget.value)} autocomplete="off" spellcheck={false} />
                            <ul class="plain models-list" aria-label={t("set.models.open")}>
                              <For each={shownModels(ok().models)} fallback={<li class="muted">{t("set.models.none")}</li>}>
                                {(m) => (
                                  <li>
                                    <button type="button" class="row model-row" disabled={!m.verifiable} aria-pressed={(model() || (o().near?.model ?? "")) === m.model} onClick={() => setModel(m.model)}>
                                      <span class="model-name"><code>{m.model}</code> <span class={`chip ${m.verifiable ? "ok" : ""}`}>{m.verifiable ? t("set.models.proof") : t("set.models.noProof")}</span><Show when={m.verifiable && m.tools === false}><span class="chip">{t("set.models.noTools")}</span></Show></span>
                                      <span class="muted">{t("set.models.cost", { input: price(m.input_per_million), output: price(m.output_per_million), currency: m.currency, window: windowText(m.context_tokens) })}</span>
                                    </button>
                                  </li>
                                )}
                              </For>
                            </ul>
                            <p class="muted">{t("set.models.note", { source: ok().source })}</p>
                          </div>
                        )}
                      </Show>
                    )}
                  </Show>
                </details>
                <label for="route-key">{t(o().near?.key_saved ? "set.route.keySaved" : "set.route.key")}</label>
                <input id="route-key" type="password" value={key()} maxlength="500" autocomplete="off" placeholder={o().near?.key_saved ? t("set.route.keyKeep") : ""} onInput={(e) => setKey(e.currentTarget.value)} />
                <div class="actions">
                  <button class="primary" type="submit" disabled={busy() || !((model().trim() || o().near?.model) && (key().trim() || o().near?.key_saved))}>{t("set.route.useNear")}</button>
                  <Show when={o().near?.key_saved && o().current !== "near"}>
                    <button type="button" disabled={busy()} onClick={() => void change({ forget_key: true }, t("set.route.done.forgot"))}>{t("set.route.forget")}</button>
                  </Show>
                </div>
              </form>
              <p class="muted">{t("set.route.note")}</p>
            </Show>
          </>
        )}
      </Show>
      <Show when={done()}><p role="status" class="ok">{done()}</p></Show>
      <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
    </div>
  );
}

/** Which installed model on this computer answers. Offered only when there is more than one and the model runs here. */
function ModelChoice(props: { id: string }) {
  const [info, { refetch }] = createResource(() => api.models());
  const [error, setError] = createSignal("");
  const [done, setDone] = createSignal("");
  const choose = async (name: string) => {
    setError("");
    setDone("");
    try {
      await api.chooseModel(name);
      setDone(t("set.model.done", { name }));
      window.dispatchEvent(new Event("kk-config-stale"));
      await refetch();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("set.model.error"));
      await refetch();
    }
  };
  return (
    <div class="stack">
      <Show when={info()} fallback={<p class="muted">{info.error ? t("set.model.error") : t("review.loading")}</p>}>
        {(m) => (
          <Show when={m().can_choose} fallback={<p class="muted">{t("set.model.fixed", { name: m().current })}</p>}>
            <select id={`set-${props.id}`} aria-labelledby={`lab-${props.id}`} value={m().current} onChange={(e) => void choose(e.currentTarget.value)}>
              {/* the model in use may not be installed: it is still shown as the one chosen, and said so in words */}
              <Show when={!m().installed.includes(m().current)}><option value={m().current} selected disabled>{t("set.model.missingOption", { name: m().current })}</option></Show>
              <For each={m().installed}>{(n) => <option value={n} selected={n === m().current}>{n}</option>}</For>
            </select>
            <Show when={!m().installed.includes(m().current)}><p class="warn">{t("set.model.missing", { name: m().current })}</p></Show>
          </Show>
        )}
      </Show>
      <Show when={done()}><p role="status" class="ok">{done()}</p></Show>
      <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
    </div>
  );
}

function Control(props: { s: Setting }) {
  const [notify, setNotify] = createSignal(notifyState());
  const flip = async (on: boolean) => {
    if (on) await turnNotifyOn();
    else notifier.setEnabled(false);
    setNotify(notifyState());
  };
  return (
    <>
      {props.s.kind === "theme" && (
        <select id={`set-${props.s.id}`} aria-labelledby={`lab-${props.s.id}`} value={theme()} onChange={(e) => chooseTheme(e.currentTarget.value as Theme)}>
          <option value="system">{t("theme.system")}</option>
          <option value="light">{t("theme.light")}</option>
          <option value="dark">{t("theme.dark")}</option>
        </select>
      )}
      {props.s.kind === "language" && (
        <select id={`set-${props.s.id}`} aria-labelledby={`lab-${props.s.id}`} value={locale()} onChange={(e) => setLocale(e.currentTarget.value as Locale)}>
          <For each={LOCALES}>{([code, name]) => <option value={code}>{name}</option>}</For>
        </select>
      )}
      {props.s.kind === "notify" && (
        <Show when={notify() !== "unsupported"} fallback={<p class="muted">{t("inbox.notify.unsupported")}</p>}>
          <label class="row-check">
            <input id={`set-${props.s.id}`} type="checkbox" checked={notify() === "on"} onChange={(e) => void flip(e.currentTarget.checked)} />
            <span>{notify() === "denied" ? t("inbox.notify.denied") : t("inbox.notify.label")}</span>
          </label>
        </Show>
      )}
      {props.s.kind === "piiallow" && <PiiAllow />}
      {props.s.kind === "hiddennames" && <HiddenNames />}
      {props.s.kind === "route" && <RouteChoice />}
      {props.s.kind === "model" && <ModelChoice id={props.s.id} />}
      {props.s.kind === "singlekeys" && (
        <label class="row-check">
          <input id={`set-${props.s.id}`} type="checkbox" checked={singleKeys()} onChange={(e) => chooseSingleKeys(e.currentTarget.checked)} />
          <span>{t("set.single.on")}</span>
        </label>
      )}
      {props.s.kind === "link" && <a class="open-link" href={props.s.target}>{t("set.open")} <span aria-hidden="true">→</span></a>}
      {props.s.kind === "shortcuts" && <button type="button" onClick={() => window.dispatchEvent(new Event("kk-show-shortcuts"))}>{t("set.keys.open")}</button>}
    </>
  );
}

/** The plain-text report of versions, checks, counts and crashes, to read, copy or save. Nothing is sent. */
function Report() {
  const [text, setText] = createSignal("");
  const [shown, setShown] = createSignal(false);
  const [note, setNote] = createSignal("");
  const [problem, setProblem] = createSignal("");
  const show = async () => {
    setProblem("");
    setNote("");
    try {
      setText((await api.diagnostics()).text);
      setShown(true);
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("diag.error"));
    }
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text());
      setNote(t("diag.copied"));
    } catch {
      setNote("");
    }
  };
  const save = () => {
    const url = URL.createObjectURL(new Blob([text()], { type: "text/plain" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = "kakurega-diagnostics.txt";
    a.click();
    URL.revokeObjectURL(url);
  };
  return (
    <section class="card stack" aria-labelledby="h-diag">
      <h2 id="h-diag">{t("diag.title")}</h2>
      <p class="muted">{t("diag.lead")}</p>
      <div class="actions">
        <Show when={!shown()} fallback={<button type="button" onClick={() => setShown(false)}>{t("diag.hide")}</button>}>
          <button type="button" onClick={() => void show()}>{t("diag.show")}</button>
        </Show>
        <Show when={shown()}>
          <button type="button" onClick={() => void copy()}>{t("diag.copy")}</button>
          <button type="button" onClick={save}>{t("diag.save")}</button>
        </Show>
      </div>
      <Show when={shown()}><pre class="passage" tabindex="0" aria-label={t("diag.title")}>{text()}</pre></Show>
      <Show when={note()}><p role="status" class="ok">{note()}</p></Show>
      <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
    </section>
  );
}

/** Every setting in one place, searchable by name, by what it does, or by a word people use for it. */
export function Settings() {
  const [q, setQ] = createSignal("");
  const found = createMemo(() => searchSettings(SETTINGS, q(), (k) => t(k as Key)));
  return (
    <section class="stack settings-page" aria-labelledby="h-set">
      <h1 id="h-set">{t("set.title")}</h1>
      <Lead text={t("set.lead")} />
      <nav class="set-sections" aria-label={t("set.sections")}>
        <For each={SECTIONS}>
          {(sec) => (
            <Show when={found().some((s) => s.section === sec)}>
              <button type="button" class="linklike" onClick={() => document.getElementById(`sec-${sec}`)?.scrollIntoView({ behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "start" })}>{t(`set.section.${sec}` as Key)}</button>
            </Show>
          )}
        </For>
      </nav>
      <label class="sr" for="set-search">{t("set.search")}</label>
      <input id="set-search" type="search" maxlength="100" placeholder={t("set.search")} value={q()} onInput={(e) => setQ(e.currentTarget.value)} />
      <Show when={q().trim() !== ""}><p class="muted" role="status">{found().length === 0 ? t("set.none") : t("set.found", { n: found().length })}</p></Show>
      <For each={SECTIONS}>
        {(sec) => (
          <Show when={found().some((s) => s.section === sec)}>
            <section class="card stack" aria-labelledby={`sec-${sec}`}>
              <h2 id={`sec-${sec}`}>{t(`set.section.${sec}` as Key)}</h2>
              <ul class="plain stack settings-list">
                <For each={found().filter((s) => s.section === sec)}>
                  {(s) => (
                    <li class="stack" id={`setting-${s.id}`}>
                      <strong id={`lab-${s.id}`}>{t(s.label as Key)}</strong>
                      <p class="muted">{t(s.help as Key)}</p>
                      <Control s={s} />
                    </li>
                  )}
                </For>
              </ul>
            </section>
          </Show>
        )}
      </For>
      <Show when={q().trim() === ""}><Report /></Show>
    </section>
  );
}
