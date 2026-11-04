import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type MemoryItem } from "../api";
import { t, type Key } from "../i18n";
import { notesFromFile } from "../notes-import";
import { Suggestions } from "./ReviewView";

const LEVELS = ["public", "internal", "confidential", "restricted"] as const;

function Item(props: { m: MemoryItem; onChange: (msg?: string) => void }) {
  const [editing, setEditing] = createSignal(false);
  const [draft, setDraft] = createSignal(props.m.text);
  const [draftProject, setDraftProject] = createSignal(props.m.project ?? "");
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
          <label for={`edit-project-${props.m.id}`}>{t("mem.add.project")}</label>
          <input id={`edit-project-${props.m.id}`} value={draftProject()} maxlength="80" placeholder={t("mem.add.projectHint")} onInput={(e) => setDraftProject(e.currentTarget.value)} />
          <div class="actions">
            <button class="primary" onClick={() => run(() => api.correctMemory(props.m.id, { text: draft(), project: draftProject().trim() }))}>{t("mem.save")}</button>
            <button onClick={() => { setEditing(false); setDraft(props.m.text); setDraftProject(props.m.project ?? ""); }}>{t("mem.cancel")}</button>
          </div>
        </div>
      }>
        <p class="mem-text">{props.m.text}</p>
        <p class="muted">
          <span class="chip">{t(`mem.level.${props.m.level}` as Key)}</span>{" "}
          <Show when={props.m.untrusted}><span class="chip warn">{t("mem.untrusted")}</span>{" "}</Show>
          <Show when={props.m.project}><span class="chip">{t("mem.project", { name: props.m.project ?? "" })}</span>{" "}</Show>
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

const MAX_NOTE_CHARS = 20000;
const MAX_NOTES = 500;

function ImportNotes(props: { onDone: () => void }) {
  const [source, setSource] = createSignal("other-assistant");
  const [picked, setPicked] = createSignal<{ name: string; text: string }[]>([]);
  const [skipped, setSkipped] = createSignal<string[]>([]);
  const [busy, setBusy] = createSignal(false);
  const [result, setResult] = createSignal("");
  const [error, setError] = createSignal("");
  const choose = async (files: FileList | null) => {
    setResult("");
    setError("");
    const ok: { name: string; text: string }[] = [];
    const bad: string[] = [];
    for (const f of Array.from(files ?? [])) {
      if (ok.length >= MAX_NOTES || f.size > MAX_NOTE_CHARS * 4 || !/\.(md|markdown|txt)$/i.test(f.name)) {
        bad.push(f.name);
        continue;
      }
      const notes = notesFromFile(f.name, await f.text());
      if (notes.length === 0 || notes.some((n) => n.text.length > MAX_NOTE_CHARS) || ok.length + notes.length > MAX_NOTES) bad.push(f.name);
      else ok.push(...notes);
    }
    setPicked(ok);
    setSkipped(bad);
  };
  const run = async () => {
    setBusy(true);
    setError("");
    setResult("");
    try {
      const r = await api.importNotes(source().trim(), picked());
      setResult(t("mem.import.result", { added: r.added, known: r.already_known, refused: r.refused, instr: r.instruction_like }));
      setPicked([]);
      props.onDone();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("mem.error"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <details class="card stack" id="mem-import">
      <summary>{t("mem.import.title")}</summary>
      <p class="muted">{t("mem.import.lead")}</p>
      <label for="imp-source">{t("mem.import.source")}</label>
      <input id="imp-source" value={source()} maxlength="40" onInput={(e) => setSource(e.currentTarget.value)} />
      <label for="imp-files">{t("mem.import.files")}</label>
      <input id="imp-files" type="file" multiple accept=".md,.markdown,.txt" onChange={(e) => void choose(e.currentTarget.files)} />
      <Show when={picked().length > 0}>
        <p><strong>{t("mem.import.will", { n: picked().length })}</strong></p>
        <ul class="plain"><For each={picked()}>{(p) => <li><code>{p.name}</code> <span class="muted">({p.text.length})</span></li>}</For></ul>
      </Show>
      <Show when={skipped().length > 0}>
        <p class="muted">{t("mem.import.skipped", { list: skipped().join(", ") })}</p>
      </Show>
      <div class="actions">
        <button class="primary" type="button" disabled={busy() || picked().length === 0 || !/^[A-Za-z0-9_-]{1,40}$/.test(source().trim())} onClick={run}>{t("mem.import.button")}</button>
        <Show when={result()}><span role="status" class="ok">{result()}</span></Show>
        <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
      </div>
    </details>
  );
}

export function Memory() {
  const [query, setQuery] = createSignal("");
  const [items, { refetch }] = createResource(query, (q) => api.memories(q.trim() || undefined).then((r) => r.memories));
  const [text, setText] = createSignal("");
  const [level, setLevel] = createSignal<string>("internal");
  const [reason, setReason] = createSignal("");
  const [project, setProject] = createSignal("");
  const [note, setNote] = createSignal("");
  const [error, setError] = createSignal("");
  const add = async (e: Event) => {
    e.preventDefault();
    setError("");
    setNote("");
    try {
      const r = await api.addMemory({ text: text(), level: level(), reason: reason().trim() || undefined, project: project().trim() || undefined });
      setNote(r.added ? t("mem.added") : t("mem.known"));
      setText("");
      setReason("");
      setProject("");
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
        <label for="mem-project">{t("mem.add.project")}</label>
        <input id="mem-project" value={project()} maxlength="80" placeholder={t("mem.add.projectHint")} onInput={(e) => setProject(e.currentTarget.value)} />
        <label for="mem-reason">{t("mem.add.reason")}</label>
        <input id="mem-reason" value={reason()} onInput={(e) => setReason(e.currentTarget.value)} />
        <div class="actions">
          <button class="primary" type="submit" disabled={text().trim().length === 0}>{t("mem.add.button")}</button>
          <Show when={note()}><span role="status" class="ok">{note()}</span></Show>
          <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
        </div>
      </form>
      <ImportNotes onDone={() => void refetch()} />
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
