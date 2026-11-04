import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type MemoryItem } from "../api";
import { t, type Key } from "../i18n";
import { Suggestions } from "./ReviewView";

const LEVELS = ["public", "internal", "confidential", "restricted"] as const;

function Item(props: { m: MemoryItem; onChange: (msg?: string) => void }) {
  const [editing, setEditing] = createSignal(false);
  const [draft, setDraft] = createSignal(props.m.text);
  const [confirming, setConfirming] = createSignal(false);
  const [showHistory, setShowHistory] = createSignal(false);
  const [history] = createResource(showHistory, (on) => (on ? api.memoryHistory(props.m.id).then((r) => r.versions) : Promise.resolve([])));
  const [error, setError] = createSignal("");
  const run = async (fn: () => Promise<unknown>, msg?: string) => {
    setError("");
    try {
      await fn();
      props.onChange(msg);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  return (
    <li class="card stack mem">
      <Show when={!editing()} fallback={
        <div class="stack">
          <label class="sr" for={`edit-${props.m.id}`}>{t("mem.correct")}</label>
          <textarea id={`edit-${props.m.id}`} rows="3" value={draft()} onInput={(e) => setDraft(e.currentTarget.value)} />
          <div class="actions">
            <button class="primary" onClick={() => run(() => api.correctMemory(props.m.id, { text: draft() }))}>{t("mem.save")}</button>
            <button onClick={() => { setEditing(false); setDraft(props.m.text); }}>{t("mem.cancel")}</button>
          </div>
        </div>
      }>
        <p class="mem-text">{props.m.text}</p>
        <p class="muted">
          <span class="chip">{t(`mem.level.${props.m.level}` as Key)}</span>{" "}
          <Show when={props.m.untrusted}><span class="chip warn">{t("mem.untrusted")}</span>{" "}</Show>
          {t("mem.why", { why: props.m.why })}
        </p>
        <div class="actions">
          <button onClick={() => setEditing(true)}>{t("mem.correct")}</button>
          <button onClick={() => setShowHistory(!showHistory())}>{showHistory() ? t("mem.history.hide") : t("mem.history")}</button>
          <Show when={!confirming()} fallback={
            <span class="confirm" role="group">
              <span class="bad">{t("mem.forget.confirm")}</span>{" "}
              <button class="danger" onClick={() => run(() => api.forgetMemory(props.m.id), t("mem.forgotten"))}>{t("mem.forget.yes")}</button>
              <button onClick={() => setConfirming(false)}>{t("mem.cancel")}</button>
            </span>
          }>
            <button onClick={() => setConfirming(true)}>{t("mem.forget")}</button>
          </Show>
        </div>
      </Show>
      <Show when={showHistory() && history()}>
        {(h) => (
          <ol class="plain">
            <For each={h()}>{(v) => <li><span class="chip">{v.current ? t("mem.history.current") : t("mem.history.old")}</span> {v.text} <span class="muted">({v.reason})</span></li>}</For>
          </ol>
        )}
      </Show>
      <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
    </li>
  );
}

export function Memory() {
  const [query, setQuery] = createSignal("");
  const [items, { refetch }] = createResource(query, (q) => api.memories(q.trim() || undefined).then((r) => r.memories));
  const [text, setText] = createSignal("");
  const [level, setLevel] = createSignal<string>("internal");
  const [reason, setReason] = createSignal("");
  const [note, setNote] = createSignal("");
  const [error, setError] = createSignal("");
  const add = async (e: Event) => {
    e.preventDefault();
    setError("");
    setNote("");
    try {
      const r = await api.addMemory({ text: text(), level: level(), reason: reason().trim() || undefined });
      setNote(r.added ? t("mem.added") : t("mem.known"));
      setText("");
      setReason("");
      await refetch();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("mem.error"));
    }
  };
  return (
    <section class="stack wide" aria-labelledby="h-mem">
      <h1 id="h-mem">{t("mem.title")}</h1>
      <p class="lead">{t("mem.lead")}</p>
      <form class="card stack" onSubmit={add}>
        <h2>{t("mem.add.heading")}</h2>
        <label for="mem-text">{t("mem.add.text")}</label>
        <textarea id="mem-text" rows="3" value={text()} onInput={(e) => setText(e.currentTarget.value)} />
        <label for="mem-level">{t("mem.add.level")}</label>
        <select id="mem-level" value={level()} onChange={(e) => setLevel(e.currentTarget.value)}>
          <For each={LEVELS}>{(l) => <option value={l}>{t(`mem.level.${l}` as Key)}</option>}</For>
        </select>
        <label for="mem-reason">{t("mem.add.reason")}</label>
        <input id="mem-reason" value={reason()} onInput={(e) => setReason(e.currentTarget.value)} />
        <div class="actions">
          <button class="primary" type="submit" disabled={text().trim().length === 0}>{t("mem.add.button")}</button>
          <Show when={note()}><span role="status" class="ok">{note()}</span></Show>
          <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
        </div>
      </form>
      <Suggestions jobId="browser" heading="sug.browser.heading" lead="sug.browser.lead" onChange={() => void refetch()} />
      <div>
        <label class="sr" for="mem-search">{t("mem.search")}</label>
        <input id="mem-search" type="search" placeholder={t("mem.search")} value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
      </div>
      <ul class="plain stack">
        <For each={items()} fallback={<li class="muted">{items.loading ? t("review.loading") : query().trim() ? t("mem.nomatch") : t("mem.none")}</li>}>
          {(m) => <Item m={m} onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />}
        </For>
      </ul>
    </section>
  );
}
