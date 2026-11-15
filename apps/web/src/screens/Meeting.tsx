import { Lead } from "../components/Lead";
import { createSignal, For, Show } from "solid-js";
import { api, ApiError, type MeetingNotes, type MeetingPoint } from "../api";
import { t } from "../i18n";
import { takeHint } from "../components/SampleCard";
import { fitBytes, NOTE_BYTES, plainTranscript } from "../meeting-text";

const KINDS = ["decision", "action", "question"] as const;
/** The most text one file may bring in: the service takes about this much in one go. */
const MAX_FILE_BYTES = 400_000;

export function Meeting() {
  const [text, setText] = createSignal(takeHint("kk-meeting-text") ?? "");
  const [title, setTitle] = createSignal(takeHint("kk-meeting-title") ?? "");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [notes, setNotes] = createSignal<MeetingNotes | null>(null);
  const [said, setSaid] = createSignal("");
  let picker: HTMLInputElement | undefined;

  const load = async (file: File | undefined) => {
    setError("");
    if (!file) return;
    if (file.size > MAX_FILE_BYTES) {
      setError(t("meet.tooBig"));
      return;
    }
    const raw = await file.text();
    setText(plainTranscript(file.name, raw));
    if (!title().trim()) setTitle(file.name.replace(/\.[^.]+$/, ""));
  };
  const write = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    setSaid("");
    setNotes(null);
    try {
      setNotes(await api.meetingNotes(text(), title().trim() || undefined));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("meet.error"));
    } finally {
      setBusy(false);
    }
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(notes()!.markdown);
      setSaid(t("meet.copied"));
    } catch {
      setSaid(t("meet.copyFailed"));
    }
  };
  const save = () => {
    const blob = new Blob([notes()!.markdown], { type: "text/markdown" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${(notes()!.title || "meeting-notes").replace(/[^\p{L}\p{N} _-]+/gu, "").trim() || "meeting-notes"}.md`;
    a.click();
    URL.revokeObjectURL(a.href);
    setSaid(t("meet.saved"));
  };
  const keep = async () => {
    try {
      const whole = notes()!.markdown;
      const text = fitBytes(whole, NOTE_BYTES);
      const r = await api.addMemory({ text, level: "confidential", reason: t("meet.noteReason") });
      setSaid(!r.added ? t("meet.keptAlready") : text.length < whole.length ? t("meet.keptPart") : t("meet.kept"));
    } catch (err) {
      setSaid(err instanceof ApiError ? err.message : t("meet.error"));
    }
  };
  const of = (n: MeetingNotes, kind: string): MeetingPoint[] => n.checked.filter((p) => p.kind === kind);

  return (
    <section class="stack" aria-labelledby="h-meet">
      <h1 id="h-meet">{t("meet.title")}</h1>
      <Lead text={t("meet.lead")} />
      <form class="card stack" onSubmit={(e) => void write(e)}>
        <label for="meet-title">{t("meet.name")}</label>
        <input id="meet-title" value={title()} maxlength="120" placeholder={t("meet.nameHint")} onInput={(e) => setTitle(e.currentTarget.value)} />
        <label for="meet-text">{t("meet.transcript")}</label>
        <textarea id="meet-text" rows="12" value={text()} placeholder={t("meet.hint")} onInput={(e) => setText(e.currentTarget.value)} />
        <div class="actions">
          <button class="primary" type="submit" disabled={busy() || text().trim().length < 40}>{busy() ? t("meet.writing") : t("meet.write")}</button>
          <input ref={picker} id="meet-file" type="file" accept=".txt,.md,.vtt,.srt,text/plain" hidden aria-label={t("meet.file")} onChange={(e) => void load(e.currentTarget.files?.[0])} />
          <button type="button" onClick={() => picker?.click()}>{t("meet.file")}</button>
          <span class="muted" aria-live="polite">{t("meet.chars", { n: text().length })}</span>
        </div>
        <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
        <Show when={busy()}><p role="status" class="muted">{t("meet.wait")}</p></Show>
      </form>
      <Show when={notes()}>
        {(n) => (
          <div class="card stack" id="meet-result" aria-labelledby="h-meet-result">
            <h2 id="h-meet-result">{n().title || t("meet.result")}</h2>
            <Show when={n().summary.length > 0}><p><span class="muted">{t("meet.summaryNote")}</span> {n().summary.join(" ")}</p></Show>
            <Show when={n().checked.length === 0 && n().unchecked.length === 0}><p class="muted">{t("meet.nothing")}</p></Show>
            <For each={KINDS}>
              {(kind) => (
                <Show when={of(n(), kind).length > 0}>
                  <h3>{t(`meet.kind.${kind}`)}</h3>
                  <ul class="stack meet-points">
                    <For each={of(n(), kind)}>
                      {(p) => (
                        <li>
                          <p><Show when={p.owner}><strong>{p.owner}: </strong></Show>{p.text}<Show when={p.due}> <span class="muted">({t("meet.due", { when: p.due })})</span></Show></p>
                          <blockquote class="quote"><span class="ok">{t("meet.checked", { line: p.line ?? 0 })}</span> {p.quote}</blockquote>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              )}
            </For>
            <Show when={n().unchecked.length > 0}>
              <h3>{t("meet.unchecked")}</h3>
              <p class="muted">{t("meet.uncheckedWhy")}</p>
              <ul class="stack meet-points">
                <For each={n().unchecked}>{(p) => <li><span class="warn">{t(`meet.kind1.${p.kind}`)}</span> {p.owner ? `${p.owner}: ` : ""}{p.text}</li>}</For>
              </ul>
            </Show>
            <ul class="plain muted">
              <li>{n().route.stays_here ? t("meet.here") : t("meet.away", { host: n().route.host ?? "" })}</li>
              <Show when={n().swapped > 0}><li>{t("meet.swapped", { n: n().swapped })}</li></Show>
              <Show when={n().withheld_lines > 0}><li>{t("meet.withheld", { n: n().withheld_lines })}</li></Show>
              <Show when={n().unreadable_parts > 0}><li class="warn">{t("meet.unreadable", { n: n().unreadable_parts, of: n().parts })}</li></Show>
              <li>{t("meet.limits")}</li>
            </ul>
            <div class="actions">
              <button type="button" onClick={() => void copy()}>{t("meet.copy")}</button>
              <button type="button" onClick={save}>{t("meet.save")}</button>
              <button type="button" onClick={() => void keep()}>{t("meet.keep")}</button>
            </div>
            <Show when={said()}><p role="status" class="ok">{said()}</p></Show>
          </div>
        )}
      </Show>
    </section>
  );
}
