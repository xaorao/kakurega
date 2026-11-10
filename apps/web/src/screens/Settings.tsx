import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError } from "../api";
import { LOCALES, locale, setLocale, t, type Key, type Locale } from "../i18n";
import { notifier, notifyState, turnNotifyOn } from "../notifier";
import { SECTIONS, SETTINGS, searchSettings, type Setting } from "../settings-catalog";
import { chooseTheme, theme, type Theme } from "../theme";
import { chooseSingleKeys, singleKeys } from "../single-keys";

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
      setDone(r.changed ? said : t("set.pii.same"));
      setEntry("");
      await refetch();
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
              <For each={m().installed}>{(n) => <option value={n} selected={n === m().current}>{n}</option>}</For>
            </select>
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
          <label class="row">
            <input id={`set-${props.s.id}`} type="checkbox" checked={notify() === "on"} onChange={(e) => void flip(e.currentTarget.checked)} />
            <span>{notify() === "denied" ? t("inbox.notify.denied") : t("inbox.notify.label")}</span>
          </label>
        </Show>
      )}
      {props.s.kind === "piiallow" && <PiiAllow />}
      {props.s.kind === "model" && <ModelChoice id={props.s.id} />}
      {props.s.kind === "singlekeys" && (
        <label class="row">
          <input id={`set-${props.s.id}`} type="checkbox" checked={singleKeys()} onChange={(e) => chooseSingleKeys(e.currentTarget.checked)} />
          <span>{t("set.single.on")}</span>
        </label>
      )}
      {props.s.kind === "link" && <a class="button" href={props.s.target}>{t("set.open")}</a>}
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
    <section class="stack wide" aria-labelledby="h-set">
      <h1 id="h-set">{t("set.title")}</h1>
      <p class="lead">{t("set.lead")}</p>
      <label class="sr" for="set-search">{t("set.search")}</label>
      <input id="set-search" type="search" maxlength="100" placeholder={t("set.search")} value={q()} onInput={(e) => setQ(e.currentTarget.value)} />
      <Show when={q().trim() !== ""}><p class="muted" role="status">{found().length === 0 ? t("set.none") : t("set.found", { n: found().length })}</p></Show>
      <For each={SECTIONS}>
        {(sec) => (
          <Show when={found().some((s) => s.section === sec)}>
            <section class="card stack" aria-labelledby={`sec-${sec}`}>
              <h2 id={`sec-${sec}`}>{t(`set.section.${sec}` as Key)}</h2>
              <ul class="plain stack">
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
