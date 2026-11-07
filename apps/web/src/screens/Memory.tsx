import { createResource, createSignal, For, Show } from "solid-js";
import { copyNames } from "../forgotten";
import { api, ApiError, type MemoryItem } from "../api";
import { t, type Key } from "../i18n";
import { isNoteFile, notesFromFile } from "../notes-import";
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
  const forgetNote = async () => {
    setError("");
    try {
      const r = await api.forgetMemory(props.m.id);
      const { shown, more } = copyNames(r);
      const parts = [t("mem.forgotten")];
      if (shown.length > 0) {
        parts.push(t("mem.forgotten.copies", { names: shown.join(", ") }));
        if (more > 0) parts.push(t("mem.forgotten.more", { n: more }));
      }
      parts.push(t("mem.forgotten.notReached"));
      props.onChange(parts.join(" "));
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
          <Show when={(props.m.uses ?? 0) > 0}><span class="chip">{props.m.uses === 1 ? t("mem.used.one") : t("mem.used.many", { n: String(props.m.uses ?? 0) })}</span>{" "}</Show>
          {t("mem.why", { why: props.m.why })}
        </p>
        <div class="actions">
          <button onClick={() => setEditing(true)}>{t("mem.correct")}</button>
          <button onClick={() => setShowHistory(!showHistory())}>{showHistory() ? t("mem.history.hide") : t("mem.history")}</button>
          <Show when={!confirming()} fallback={
            <span class="confirm" role="group">
              <span class="bad">{t("mem.forget.confirm")}</span>{" "}
              <button class="danger" onClick={() => forgetNote()}>{t("mem.forget.yes")}</button>
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
      if (ok.length >= MAX_NOTES || f.size > MAX_NOTE_CHARS * 4 || !isNoteFile(f.name)) {
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
      <input id="imp-files" type="file" multiple accept=".md,.mdc,.markdown,.txt" onChange={(e) => void choose(e.currentTarget.files)} />
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

function Similar(props: { onChange: (msg?: string) => void }) {
  const [open, setOpen] = createSignal(false);
  const [found, { refetch }] = createResource(open, (on) => (on ? api.similarMemories() : Promise.resolve(null)));
  const [problem, setProblem] = createSignal("");
  const merge = async (keep: string, others: string[]) => {
    setProblem("");
    try {
      const r = await api.mergeMemories(keep, others);
      props.onChange(t("mem.merge.done", { n: r.merged }));
      await refetch();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  return (
    <details class="card stack" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>{t("mem.similar.title")}</summary>
      <Show when={open()}>
        <p class="muted">{t("mem.similar.lead")}</p>
        <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
        <Show when={found()} fallback={<p class="muted">{t("review.loading")}</p>}>
          {(f) => (
            <>
              <p class="muted">{t("mem.similar.checked", { n: f().scanned })}{" "}{f().complete ? "" : t("mem.similar.partial", { n: f().scanned })}</p>
              <Show when={f().groups.length > 0} fallback={<p>{t("mem.similar.none")}</p>}>
                <For each={f().groups}>
                  {(g) => (
                    <div class="stack">
                      <p><strong>{t("mem.similar.group", { n: g.length })}</strong></p>
                      <ul class="plain stack">
                        <For each={g}>
                          {(m) => (
                            <>
                              <Item m={m} onChange={(msg) => { props.onChange(msg); void refetch(); }} />
                              <li class="actions">
                                <button type="button" aria-label={t("mem.merge.keepFor", { text: m.text.slice(0, 40) })} onClick={() => void merge(m.id, g.filter((o) => o.id !== m.id).map((o) => o.id).slice(0, 8))}>{t("mem.merge.keep")}</button>
                              </li>
                            </>
                          )}
                        </For>
                      </ul>
                    </div>
                  )}
                </For>
              </Show>
            </>
          )}
        </Show>
      </Show>
    </details>
  );
}

function Held(props: { onChange: (msg?: string) => void }) {
  const [list, { refetch }] = createResource(() => api.heldMemories().then((r) => r.held).catch(() => []));
  const [problem, setProblem] = createSignal("");
  const [confirming, setConfirming] = createSignal<string | null>(null);
  const done = async (msg: string) => {
    props.onChange(msg);
    setConfirming(null);
    await refetch();
  };
  const release = async (id: string) => {
    setProblem("");
    try {
      await api.releaseHold(id);
      await done(t("mem.held.released"));
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  const forget = async (id: string) => {
    setProblem("");
    try {
      await api.forgetMemory(id, "held as a possible secret");
      await done(t("mem.forgotten"));
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  return (
    <Show when={(list() ?? []).length > 0}>
      <section class="card stack" role="region" aria-labelledby="h-held">
        <h2 id="h-held">{t("mem.held.title", { n: (list() ?? []).length })}</h2>
        <p class="muted">{t("mem.held.lead")}</p>
        <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
        <ul class="plain stack">
          <For each={list() ?? []}>
            {(m) => (
              <li class="card stack">
                <p><span class="chip warn">{t("mem.held.kind", { kind: m.kind })}</span></p>
                <p>{m.text}</p>
                <div class="actions">
                  <Show when={confirming() === m.id} fallback={<>
                    <button type="button" onClick={() => void release(m.id)}>{t("mem.held.notSecret")}</button>
                    <button type="button" onClick={() => setConfirming(m.id)}>{t("mem.forget")}</button>
                  </>}>
                    <span class="bad">{t("mem.forget.confirm")}</span>{" "}
                    <button class="danger" onClick={() => void forget(m.id)}>{t("mem.forget.yes")}</button>
                    <button type="button" onClick={() => setConfirming(null)}>{t("mem.cancel")}</button>
                  </Show>
                </div>
              </li>
            )}
          </For>
        </ul>
      </section>
    </Show>
  );
}

function Merged(props: { onChange: (msg?: string) => void }) {
  const [open, setOpen] = createSignal(false);
  const [list, { refetch }] = createResource(open, (on) => (on ? api.mergedMemories().then((r) => r.merged) : Promise.resolve([])));
  const [problem, setProblem] = createSignal("");
  const back = async (id: string) => {
    setProblem("");
    try {
      await api.unmergeMemory(id);
      props.onChange(t("mem.merged.restored"));
      await refetch();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  return (
    <details class="card stack" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>{t("mem.merged.title")}</summary>
      <Show when={open()}>
        <p class="muted">{t("mem.merged.lead")}</p>
        <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
        <Show when={(list() ?? []).length > 0} fallback={<p role="status">{t("mem.merged.none")}</p>}>
          <ul class="plain stack">
            <For each={list() ?? []}>
              {(m) => (
                <li class="card stack">
                  <p>{m.text}</p>
                  <div class="actions"><button type="button" onClick={() => void back(m.id)}>{t("mem.merged.back")}</button></div>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </details>
  );
}

/** Notes that disagree: the owner keeps one (the other stays as its earlier version) or says both are right. */
function Disagreements(props: { onChange: (msg?: string) => void }) {
  const [open, setOpen] = createSignal(false);
  const [found, { refetch }] = createResource(open, (on) => (on ? api.memoryConflicts() : Promise.resolve(null)));
  const [problem, setProblem] = createSignal("");
  const act = async (fn: () => Promise<unknown>, msg: string) => {
    setProblem("");
    try {
      await fn();
      props.onChange(msg);
      await refetch();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  return (
    <details class="card stack" id="mem-conflicts" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>{t("mem.conf.title")}</summary>
      <Show when={open()}>
        <p class="muted">{t("mem.conf.lead")}</p>
        <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
        <Show when={found()} fallback={<p class="muted">{t("review.loading")}</p>}>
          {(f) => (
            <>
              <p class="muted">{t("mem.similar.checked", { n: f().scanned })}{" "}{f().complete ? "" : t("mem.similar.partial", { n: f().scanned })}</p>
              <Show when={f().pairs.length > 0} fallback={<p>{t("mem.conf.none")}</p>}>
                <ul class="plain stack">
                  <For each={f().pairs}>
                    {(p) => (
                      <li class="card stack">
                        <p class="muted">{p.why === "same_subject" ? t("mem.conf.subject", { subject: p.subject ?? "" }) : t("mem.conf.words")}</p>
                        <For each={[p.a, p.b]}>
                          {(m) => (
                            <div class="stack">
                              <p class="mem-text">{m.text}</p>
                              <div class="actions">
                                <button type="button" aria-label={t("mem.conf.keepFor", { text: m.text.slice(0, 40) })} onClick={() => void act(() => api.keepNote(m.id, (m.id === p.a.id ? p.b : p.a).id), t("mem.conf.kept"))}>{t("mem.conf.keep")}</button>
                              </div>
                            </div>
                          )}
                        </For>
                        <div class="actions">
                          <button type="button" onClick={() => void act(() => api.bothRight(p.a.id, p.b.id), t("mem.conf.dismissed"))}>{t("mem.conf.both")}</button>
                        </div>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </>
          )}
        </Show>
      </Show>
    </details>
  );
}

/** Everything one review left in memory (notes and waiting suggestions), forgotten in one step. */
function ReviewSources(props: { onChange: (msg?: string) => void }) {
  const [open, setOpen] = createSignal(false);
  const [sources, { refetch }] = createResource(open, (on) => (on ? api.reviewSources().then((r) => r.sources) : Promise.resolve([])));
  const [confirming, setConfirming] = createSignal("");
  const [problem, setProblem] = createSignal("");
  const forget = async (job: string) => {
    setProblem("");
    try {
      const r = await api.forgetReview(job);
      setConfirming("");
      props.onChange(t("mem.src.done", { notes: r.notes, sugg: r.suggestions }));
      await refetch();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  return (
    <details class="card stack" id="mem-sources" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>{t("mem.src.title")}</summary>
      <Show when={open()}>
        <p class="muted">{t("mem.src.lead")}</p>
        <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
        <ul class="plain stack">
          <For each={sources()} fallback={<li class="muted">{sources.loading ? t("review.loading") : t("mem.src.none")}</li>}>
            {(s) => (
              <li class="card stack">
                <p><code>{s.job}</code> <span class="muted">{t("mem.src.counts", { notes: s.notes, sugg: s.suggestions })}</span></p>
                <div class="actions">
                  <Show when={confirming() !== s.job} fallback={
                    <span class="confirm" role="group">
                      <span class="bad">{t("mem.src.confirm", { notes: s.notes, sugg: s.suggestions })}</span>{" "}
                      <button class="danger" onClick={() => void forget(s.job)}>{t("mem.src.yes")}</button>
                      <button onClick={() => setConfirming("")}>{t("mem.cancel")}</button>
                    </span>
                  }>
                    <button type="button" onClick={() => setConfirming(s.job)}>{t("mem.src.forget")}</button>
                  </Show>
                </div>
              </li>
            )}
          </For>
        </ul>
      </Show>
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
      <Held onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />
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
      <Similar onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />
      <Disagreements onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />
      <Merged onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />
      <ReviewSources onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />
      <ul class="plain stack">
        <For each={items()} fallback={<li class="muted">{items.loading ? t("review.loading") : query().trim() ? t("mem.nomatch") : t("mem.none")}</li>}>
          {(m) => <Item m={m} onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />}
        </For>
      </ul>
    </section>
  );
}
