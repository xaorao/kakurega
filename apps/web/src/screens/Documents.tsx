import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type AskReply, type Passage } from "../api";
import { t, type Key } from "../i18n";

export function Documents() {
  const [dir, setDir] = createSignal<string | undefined>(undefined);
  const [listing] = createResource(() => dir() ?? "", (d) => api.fs(d || undefined));
  const [question, setQuestion] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [reply, setReply] = createSignal<AskReply | null>(null);
  const chosen = () => listing()?.path ?? "";
  const ask = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    setReply(null);
    try {
      setReply(await api.ask(chosen(), question().trim()));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("docs.error"));
    } finally {
      setBusy(false);
    }
  };
  const [passages, setPassages] = createSignal<Record<string, Passage | string>>({});
  const showPassage = async (file: string, line: number, folder: string) => {
    const key = `${file}:${line}`;
    if (passages()[key]) {
      const { [key]: _gone, ...rest } = passages();
      setPassages(rest);
      return;
    }
    try {
      setPassages({ ...passages(), [key]: await api.passage(folder, file, line) });
    } catch (err) {
      setPassages({ ...passages(), [key]: err instanceof ApiError ? err.message : t("docs.error") });
    }
  };
  const list = (xs: string[]) => xs.join(", ");
  return (
    <section class="stack wide" aria-labelledby="h-docs">
      <h1 id="h-docs">{t("docs.title")}</h1>
      <p class="lead">{t("docs.lead")}</p>
      <div class="card stack">
        <h2>{t("docs.step1")}</h2>
        <p class="path" aria-live="polite">{chosen() || t("new.loading")}</p>
        <div class="browser" role="group" aria-label={t("new.folders")}>
          <Show when={listing()?.parent}>{(p) => <button class="row" onClick={() => setDir(p())}>{t("new.up")}</button>}</Show>
          <For each={listing()?.dirs} fallback={<p class="muted">{listing.loading ? t("new.loadingFolders") : t("new.noFolders")}</p>}>
            {(d) => <button class="row" onClick={() => setDir(d.path)}>{d.name}</button>}
          </For>
        </div>
      </div>
      <form class="card stack" onSubmit={ask}>
        <h2>{t("docs.step2")}</h2>
        <label for="docs-q">{t("docs.question")}</label>
        <input id="docs-q" value={question()} onInput={(e) => setQuestion(e.currentTarget.value)} maxLength={500} />
        <div class="actions">
          <button class="primary" type="submit" disabled={busy() || !chosen() || question().trim().length === 0}>{t("docs.ask")}</button>
          <Show when={busy()}><span role="status" class="muted">{t("docs.asking")}</span></Show>
          <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
        </div>
      </form>
      <Show when={reply()}>
        {(r) => (
          <section class="card stack" aria-labelledby="h-answer">
            <h2 id="h-answer">{t("docs.answer")}</h2>
            <Show when={r().checkable} fallback={<><p class="warn">{t("docs.unchecked")}</p><p class="mem-text">{r().unchecked_text}</p></>}>
              <p class="mem-text">{r().answer}</p>
              <p class="muted">{t("docs.notProof")}</p>
              <h3>{t("docs.checked")}</h3>
              <Show when={(r().citations ?? []).length > 0} fallback={<p class="muted">{t("docs.noQuotes")}</p>}>
                <p><strong>{t("docs.count", { v: r().verified ?? 0, n: (r().citations ?? []).length })}</strong></p>
                <ul class="plain stack">
                  <For each={r().citations}>
                    {(c) => (
                      <li class="stack cite">
                        <p class="mem-text">&ldquo;{c.quote}&rdquo;</p>
                        <p class="muted"><code>{c.file}</code> <span class={`chip ${c.verdict === "verified" ? "ok" : "bad"}`}>{t(`docs.${c.verdict}` as Key, { line: c.line ?? 0 })}</span>
                          <Show when={c.verdict === "verified" && c.line}>
                            {" "}<button type="button" class="link" aria-expanded={!!passages()[`${c.file}:${c.line}`]} onClick={() => showPassage(c.file, c.line as number, chosen())}>{passages()[`${c.file}:${c.line}`] ? t("docs.hidePassage") : t("docs.showPassage")}</button>
                          </Show>
                        </p>
                        <Show when={passages()[`${c.file}:${c.line}`]}>
                          {(p) => (
                            <Show when={typeof p() !== "string"} fallback={<p role="alert" class="bad">{String(p())}</p>}>
                              <pre class="passage" tabindex="0" aria-label={t("docs.passageLabel", { file: c.file })}>
                                <For each={(p() as Passage).lines}>{(l, i) => <div class={(p() as Passage).start + i() === (p() as Passage).focus ? "hit" : ""}><span class="ln">{(p() as Passage).start + i()}</span> {l}</div>}</For>
                              </pre>
                            </Show>
                          )}
                        </Show>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </Show>
            <p class="muted">{t("docs.used", { list: list(r().files_used) })}</p>
            <Show when={r().files_withheld.length > 0}><p class="muted">{t("docs.withheld", { list: list(r().files_withheld) })}</p></Show>
            <Show when={r().files_skipped.length > 0}><p class="muted">{t("docs.skipped", { list: list(r().files_skipped) })}</p></Show>
            <Show when={r().injection_suspects.length > 0}><p class="warn">{t("docs.injection", { list: list(r().injection_suspects) })}</p></Show>
          </section>
        )}
      </Show>
    </section>
  );
}
