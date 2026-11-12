import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { copyNames, leftBehind, type LeftBehind } from "../forgotten";
import { api, ApiError, type MemoryItem } from "../api";
import { t, type Key } from "../i18n";
import { isNoteFile, notesFromFile } from "../notes-import";
import { Suggestions } from "./ReviewView";
import { markTerms } from "../terms";

const LEVELS = ["public", "internal", "confidential", "restricted"] as const;

function Item(props: { m: MemoryItem; pinned?: boolean; onChange: (msg?: string) => void; onLeft?: (l: LeftBehind | null) => void }) {
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
      props.onLeft?.(leftBehind(r));
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
        <p class="mem-text">
          <For each={markTerms(props.m.text, props.m.matched_terms ?? [])}>{(piece) => (piece.hit ? <mark>{piece.text}</mark> : <>{piece.text}</>)}</For>
        </p>
        <p class="muted">
          <span class="chip">{t(`mem.level.${props.m.level}` as Key)}</span>{" "}
          <Show when={props.m.untrusted}><span class="chip warn">{t("mem.untrusted")}</span>{" "}</Show>
          <Show when={props.m.instruction_like}><span class="chip warn">{t("mem.instructionLike")}</span>{" "}</Show>
          <Show when={props.m.origin === "imported" || props.m.origin === "conversation" || props.m.origin === "review"}><span class="chip">{t(`mem.origin.${props.m.origin}` as Key)}</span>{" "}</Show>
          <Show when={props.m.project}><span class="chip">{t("mem.project", { name: props.m.project ?? "" })}</span>{" "}</Show>
          <Show when={props.pinned}><span class="chip">{t("mem.pinned")}</span>{" "}</Show>
          <Show when={(props.m.uses ?? 0) > 0}><span class="chip">{props.m.uses === 1 ? t("mem.used.one") : t("mem.used.many", { n: String(props.m.uses ?? 0) })}</span>{" "}</Show>
          {t("mem.why", { why: props.m.why })}
        </p>
        <div class="actions">
          <button onClick={() => setEditing(true)}>{t("mem.correct")}</button>
          <button onClick={() => setShowHistory(!showHistory())}>{showHistory() ? t("mem.history.hide") : t("mem.history")}</button>
          <button onClick={() => run(() => api.keepMemory(props.m.id, props.pinned ? "unpin" : "pin"), props.pinned ? t("mem.unpinDone") : t("mem.pinDone"))}>{props.pinned ? t("mem.unpin") : t("mem.pin")}</button>
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

/** Bytes as URL-safe base64 without padding, the form the service speaks, in pieces so a large file does not overflow the stack. */
function toBase64Url(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i += 0x8000) out += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(out).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** The bytes of URL-safe base64 text without padding, as the service writes it. */
function fromBase64Url(text: string): Uint8Array<ArrayBuffer> {
  const standard = text.replace(/-/g, "+").replace(/_/g, "/");
  const raw = atob(standard + "=".repeat((4 - (standard.length % 4)) % 4));
  const bytes = new Uint8Array(new ArrayBuffer(raw.length));
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

function download(name: string, type: string, data: BlobPart) {
  const url = URL.createObjectURL(new Blob([data], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** The largest backup file restored here: about 24 MB as base64 text is well under what the service accepts (32 MiB). */
const MAX_BACKUP_BYTES = 22 * 1024 * 1024;

function NotesBackup(props: { onDone: () => void }) {
  const [busy, setBusy] = createSignal(false);
  const [message, setMessage] = createSignal("");
  const [problem, setProblem] = createSignal("");
  const [key, setKey] = createSignal("");
  const [copied, setCopied] = createSignal(false);
  const [picked, setPicked] = createSignal<{ sealed: boolean; b64: string; doc: unknown } | null>(null);
  const [typedKey, setTypedKey] = createSignal("");
  const fail = (e: unknown) => setProblem(e instanceof ApiError ? e.message : t("mem.error"));
  const readable = async () => {
    setBusy(true); setProblem(""); setMessage("");
    try {
      const doc = (await api.exportNotesPlain()) as { memories: unknown[]; archived?: unknown[] };
      download("kakurega-memory-backup.json", "application/json", JSON.stringify(doc));
      setMessage(t("mem.backup.readableDone", { n: String((doc.memories?.length ?? 0) + (doc.archived?.length ?? 0)) }));
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  const protectedFile = async () => {
    setBusy(true); setProblem(""); setMessage(""); setKey(""); setCopied(false);
    try {
      const r = await api.exportNotesSealed();
      download(r.filename, "application/octet-stream", fromBase64Url(r.file));
      setKey(r.recovery_key);
      setMessage(t("mem.backup.protectedDone", { n: String(r.notes) }));
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  const copyKey = async () => {
    try { await navigator.clipboard.writeText(key()); setCopied(true); } catch { /* the key stays on screen to be copied by hand */ }
  };
  const choose = async (file: File | undefined) => {
    setPicked(null); setProblem(""); setMessage(""); setTypedKey("");
    if (!file) return;
    if (file.size > MAX_BACKUP_BYTES) { setProblem(t("mem.backup.tooBig")); return; }
    const bytes = new Uint8Array(await file.arrayBuffer());
    const magic = new TextDecoder().decode(bytes.subarray(0, 8));
    if (magic === "KKBACKUP") { setPicked({ sealed: true, b64: toBase64Url(bytes), doc: null }); return; }
    try {
      const doc = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
      if (doc && typeof doc === "object" && (doc as { kakurega_memory_backup?: unknown }).kakurega_memory_backup === 1) { setPicked({ sealed: false, b64: "", doc }); return; }
    } catch { /* falls through to the message below */ }
    setProblem(t("mem.backup.unreadable"));
  };
  const restore = async () => {
    const p = picked();
    if (!p) return;
    setBusy(true); setProblem(""); setMessage("");
    try {
      const r = p.sealed ? await api.restoreNotesSealed(p.b64, typedKey().trim()) : await api.restoreNotes(p.doc);
      setMessage(t("mem.backup.restored", { added: String(r.added), known: String(r.already_known), refused: String(r.refused) }) + ((r.not_saved ?? 0) > 0 ? " " + t("mem.notSaved", { n: r.not_saved ?? 0 }) : ""));
      setPicked(null); setTypedKey("");
      props.onDone();
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  return (
    <details class="card stack" id="mem-backup">
      <summary>{t("mem.backup.title")}</summary>
      <p class="muted">{t("mem.backup.lead")}</p>
      <div class="actions">
        <button type="button" disabled={busy()} onClick={() => void readable()}>{t("mem.backup.readable")}</button>
        <button type="button" disabled={busy()} onClick={() => void protectedFile()}>{t("mem.backup.protected")}</button>
      </div>
      <Show when={key()}>
        <section class="card stack" role="region" aria-label={t("mem.backup.keyTitle")}>
          <h3>{t("mem.backup.keyTitle")}</h3>
          <p class="bad">{t("mem.backup.keyWarn")}</p>
          <p><code class="mono" id="mem-backup-key">{key()}</code></p>
          <div class="actions">
            <button type="button" onClick={() => void copyKey()}>{copied() ? t("mem.backup.keyCopied") : t("mem.backup.keyCopy")}</button>
            <button type="button" class="primary" onClick={() => { setKey(""); setCopied(false); }}>{t("mem.backup.keyDone")}</button>
          </div>
        </section>
      </Show>
      <h3>{t("mem.backup.restore")}</h3>
      <label for="mem-backup-file">{t("mem.backup.file")}</label>
      <input id="mem-backup-file" type="file" accept=".json,.kkmem,application/json" onChange={(e) => void choose(e.currentTarget.files?.[0])} />
      <Show when={picked()?.sealed}>
        <p>{t("mem.backup.needsKey")}</p>
        <label for="mem-backup-typed">{t("mem.backup.keyLabel")}</label>
        <input id="mem-backup-typed" autocomplete="off" spellcheck={false} value={typedKey()} onInput={(e) => setTypedKey(e.currentTarget.value)} />
      </Show>
      <Show when={picked()}>
        <div class="actions"><button type="button" class="primary" disabled={busy() || (picked()?.sealed === true && typedKey().trim() === "")} onClick={() => void restore()}>{t("mem.backup.restoreButton")}</button></div>
      </Show>
      <Show when={message()}><p role="status">{message()}</p></Show>
      <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
    </details>
  );
}

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
      setResult(t("mem.import.result", { added: r.added, known: r.already_known, refused: r.refused, instr: r.instruction_like }) + ((r.not_saved ?? 0) > 0 ? " " + t("mem.notSaved", { n: r.not_saved ?? 0 }) : ""));
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

function Similar(props: { onChange: (msg?: string) => void; onLeft?: (l: LeftBehind | null) => void }) {
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
                              <Item m={m} onChange={(msg) => { props.onChange(msg); void refetch(); }} onLeft={props.onLeft} />
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

/** The conversations and typed history that still hold a note the owner just forgot, each removable on its own and only on request. */
function LeftBehindCard(props: { left: LeftBehind; onDone: () => void }) {
  const [titles] = createResource(() => api.keptChats(true).then((r) => new Map(r.sessions.map((s) => [s.id, s.title]))).catch(() => new Map<string, string>()));
  const [gone, setGone] = createSignal<string[]>([]);
  const [historyCleared, setHistoryCleared] = createSignal(false);
  const [status, setStatus] = createSignal("");
  const [problem, setProblem] = createSignal("");
  const remaining = () => props.left.conversations.filter((c) => !gone().includes(c.id));
  const deleteChat = async (id: string) => {
    setProblem("");
    try {
      await api.forgetChat(id);
      setGone([...gone(), id]);
      setStatus(t("mem.left.deleted"));
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  const clearHistory = async () => {
    setProblem("");
    try {
      await api.clearTypedHistory();
      setHistoryCleared(true);
      setStatus(t("mem.left.historyCleared"));
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  return (
    <section class="card stack" role="region" aria-labelledby="h-left">
      <h2 id="h-left">{t("mem.left.heading")}</h2>
      <p class="muted">{t("mem.left.lead")}</p>
      <Show when={props.left.incomplete}><p class="warn">{t("mem.left.incomplete")}</p></Show>
      <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
      <p role="status" class="sr">{status()}</p>
      <ul class="plain stack">
        <For each={remaining()}>
          {(c) => (
            <li class="card stack">
              <p><strong>{titles()?.get(c.id) || t("mem.left.untitled")}</strong></p>
              <p class="muted">{c.why === "source" ? t("mem.left.source") : t("mem.left.text")}</p>
              <div class="actions">
                <button type="button" class="danger" aria-label={t("mem.left.deleteLabel", { title: titles()?.get(c.id) || t("mem.left.untitled") })} onClick={() => void deleteChat(c.id)}>{t("mem.left.delete")}</button>
              </div>
            </li>
          )}
        </For>
      </ul>
      <Show when={props.left.more > 0}><p class="muted">{t("mem.forgotten.more", { n: props.left.more })}</p></Show>
      <Show when={props.left.history > 0 && !historyCleared()}>
        <div class="card stack">
          <p>{t("mem.left.history", { n: props.left.history })}</p>
          <div class="actions"><button type="button" class="danger" onClick={() => void clearHistory()}>{t("mem.left.clearHistory")}</button></div>
        </div>
      </Show>
      <div class="actions"><button type="button" onClick={props.onDone}>{t("mem.left.close")}</button></div>
    </section>
  );
}

function Held(props: { onChange: (msg?: string) => void; onLeft?: (l: LeftBehind | null) => void }) {
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
      const r = await api.forgetMemory(id, "held as a possible secret");
      props.onLeft?.(leftBehind(r));
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

function Quiet(props: { onChange: (msg?: string) => void }) {
  const [open, setOpen] = createSignal(false);
  const [days, setDays] = createSignal(180);
  const [list, { refetch }] = createResource(() => (open() ? days() : 0), (d) => (d ? api.dormantMemories(d).then((r) => r.dormant) : Promise.resolve([])));
  const [problem, setProblem] = createSignal("");
  const act = async (id: string, action: "archive" | "keep" | "pin", done: Key) => {
    setProblem("");
    try {
      await api.keepMemory(id, action);
      props.onChange(t(done));
      await refetch();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  return (
    <details class="card stack" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>{t("mem.quiet.title")}</summary>
      <Show when={open()}>
        <p class="muted">{t("mem.quiet.lead")}</p>
        <label for="mem-quiet-days">{t("mem.quiet.days")}</label>
        <select id="mem-quiet-days" value={String(days())} onChange={(e) => setDays(Number(e.currentTarget.value))}>
          <For each={[30, 90, 180, 365, 730]}>{(d) => <option value={String(d)}>{t("mem.quiet.option", { n: String(d) })}</option>}</For>
        </select>
        <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
        <Show when={(list() ?? []).length > 0} fallback={<p role="status">{list.loading ? t("review.loading") : t("mem.quiet.none")}</p>}>
          <ul class="plain stack">
            <For each={list() ?? []}>
              {(m) => (
                <li class="card stack">
                  <p>{m.text}</p>
                  <p class="muted">{t("mem.quiet.line", { days: String(m.idle_days), uses: String(m.uses) })}</p>
                  <div class="actions">
                    <button type="button" onClick={() => void act(m.id, "archive", "mem.quiet.awayDone")}>{t("mem.quiet.away")}</button>
                    <button type="button" onClick={() => void act(m.id, "keep", "mem.quiet.keepDone")}>{t("mem.quiet.keep")}</button>
                    <button type="button" onClick={() => void act(m.id, "pin", "mem.pinDone")}>{t("mem.quiet.pin")}</button>
                  </div>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </details>
  );
}

function Archived(props: { onChange: (msg?: string) => void; onLeft?: (l: LeftBehind | null) => void }) {
  const [open, setOpen] = createSignal(false);
  const [list, { refetch }] = createResource(open, (on) => (on ? api.archivedMemories().then((r) => r.archived) : Promise.resolve([])));
  const [problem, setProblem] = createSignal("");
  const [confirming, setConfirming] = createSignal("");
  const back = async (id: string) => {
    setProblem("");
    try {
      await api.keepMemory(id, "restore");
      props.onChange(t("mem.archived.restored"));
      await refetch();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  const forget = async (id: string) => {
    setProblem("");
    try {
      const r = await api.forgetMemory(id);
      const { shown, more } = copyNames(r);
      const parts = [t("mem.forgotten")];
      if (shown.length > 0) {
        parts.push(t("mem.forgotten.copies", { names: shown.join(", ") }));
        if (more > 0) parts.push(t("mem.forgotten.more", { n: more }));
      }
      parts.push(t("mem.forgotten.notReached"));
      props.onLeft?.(leftBehind(r));
      props.onChange(parts.join(" "));
      setConfirming("");
      await refetch();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mem.error"));
    }
  };
  return (
    <details class="card stack" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>{t("mem.archived.title")}</summary>
      <Show when={open()}>
        <p class="muted">{t("mem.archived.lead")}</p>
        <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
        <Show when={(list() ?? []).length > 0} fallback={<p role="status">{t("mem.archived.none")}</p>}>
          <ul class="plain stack">
            <For each={list() ?? []}>
              {(m) => (
                <li class="card stack">
                  <p>{m.text}</p>
                  <div class="actions">
                    <button type="button" onClick={() => void back(m.id)}>{t("mem.archived.back")}</button>
                    <Show when={confirming() !== m.id} fallback={
                      <span class="confirm" role="group">
                        <span class="bad">{t("mem.forget.confirm")}</span>{" "}
                        <button class="danger" onClick={() => void forget(m.id)}>{t("mem.forget.yes")}</button>
                        <button onClick={() => setConfirming("")}>{t("mem.cancel")}</button>
                      </span>
                    }>
                      <button type="button" onClick={() => setConfirming(m.id)}>{t("mem.forget")}</button>
                    </Show>
                  </div>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </details>
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
  const [fLevel, setFLevel] = createSignal("");
  const [fOrigin, setFOrigin] = createSignal("");
  const [fProject, setFProject] = createSignal("");
  const [fUnused, setFUnused] = createSignal(false);
  const [fEnding, setFEnding] = createSignal(false);
  const filters = createMemo(() => ({ q: query(), level: fLevel(), origin: fOrigin(), project: fProject(), unused: fUnused(), ending: fEnding() }));
  const filtering = () => !!(fLevel() || fOrigin() || fProject() || fUnused() || fEnding());
  const [items, { refetch: refetchItems }] = createResource(filters, (f) => api.memories(f.q.trim() || undefined, { level: f.level || undefined, origin: f.origin || undefined, project: f.project || undefined, unused: f.unused, ending: f.ending }).then((r) => r.memories));
  const [stats, { refetch: refetchStats }] = createResource(() => api.memoryStats().catch(() => null));
  const [pinned, { refetch: refetchPinned }] = createResource(() => api.pinnedMemories().then((r) => new Set(r.pinned)).catch(() => new Set<string>()));
  const refetch = async () => {
    await Promise.all([refetchItems(), refetchStats(), refetchPinned()]);
  };
  const [text, setText] = createSignal("");
  const [level, setLevel] = createSignal<string>("internal");
  const [reason, setReason] = createSignal("");
  const [project, setProject] = createSignal("");
  const [note, setNote] = createSignal("");
  const [left, setLeft] = createSignal<LeftBehind | null>(null);
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
      <Show when={left()}>{(l) => <LeftBehindCard left={l()} onDone={() => setLeft(null)} />}</Show>
      <Held onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} onLeft={setLeft} />
      <Suggestions jobId="browser" heading="sug.browser.heading" lead="sug.browser.lead" onChange={() => void refetch()} />
      <div>
        <label class="sr" for="mem-search">{t("mem.search")}</label>
        <input id="mem-search" type="search" placeholder={t("mem.search")} value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
      </div>
      <section class="card stack" aria-labelledby="h-mem-glance">
        <h2 id="h-mem-glance">{t("mem.glance.heading")}</h2>
        <Show when={stats()} fallback={<p class="muted">{t("mem.glance.unknown")}</p>}>
          {(s) => (
            <p role="status">{t("mem.glance.line", { total: s().total, untrusted: s().untrusted, unused: s().never_used, soon: s().ending_within_14_days, held: s().held })}</p>
          )}
        </Show>
        <div class="actions">
          <label for="mem-f-level">{t("mem.filter.level")}</label>
          <select id="mem-f-level" value={fLevel()} onChange={(e) => setFLevel(e.currentTarget.value)}>
            <option value="">{t("mem.filter.any")}</option>
            <For each={LEVELS}>{(l) => <option value={l}>{t(`mem.level.${l}` as Key)}</option>}</For>
          </select>
          <label for="mem-f-origin">{t("mem.filter.origin")}</label>
          <select id="mem-f-origin" value={fOrigin()} onChange={(e) => setFOrigin(e.currentTarget.value)}>
            <option value="">{t("mem.filter.any")}</option>
            <For each={["owner", "imported", "conversation", "review"]}>{(o) => <option value={o}>{t(`mem.origin.${o}` as Key)}</option>}</For>
          </select>
          <Show when={Object.keys(stats()?.by_project ?? {}).length > 0}>
            <label for="mem-f-project">{t("mem.filter.project")}</label>
            <select id="mem-f-project" value={fProject()} onChange={(e) => setFProject(e.currentTarget.value)}>
              <option value="">{t("mem.filter.any")}</option>
              <For each={Object.keys(stats()?.by_project ?? {}).sort()}>{(p) => <option value={p}>{p}</option>}</For>
            </select>
          </Show>
          <label class="choice"><input type="checkbox" checked={fUnused()} onChange={(e) => setFUnused(e.currentTarget.checked)} /> {t("mem.filter.unused")}</label>
          <label class="choice"><input type="checkbox" checked={fEnding()} onChange={(e) => setFEnding(e.currentTarget.checked)} /> {t("mem.filter.ending")}</label>
          <Show when={filtering()}><button type="button" onClick={() => { setFLevel(""); setFOrigin(""); setFProject(""); setFUnused(false); setFEnding(false); }}>{t("mem.filter.reset")}</button></Show>
        </div>
      </section>
      <ul class="plain stack">
        <For each={items()} fallback={<li class="muted">{items.loading ? t("review.loading") : query().trim() || filtering() ? t("mem.nomatch") : t("mem.none")}</li>}>
          {(m) => <Item m={m} pinned={pinned()?.has(m.id)} onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} onLeft={setLeft} />}
        </For>
      </ul>
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
      {/* the notes come first; looking after them (duplicates, disagreements, what was put away, bringing notes in, backups) comes after */}
      <Similar onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} onLeft={setLeft} />
      <Disagreements onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />
      <Merged onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />
      <Quiet onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />
      <Archived onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} onLeft={setLeft} />
      <ReviewSources onChange={(msg) => { setNote(msg ?? ""); void refetch(); }} />
      <ImportNotes onDone={() => void refetch()} />
      <NotesBackup onDone={() => void refetch()} />
    </section>
  );
}
