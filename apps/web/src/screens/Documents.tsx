import { LoadState } from "../components/LoadState";
import { takeHint } from "../components/SampleCard";
import { visible } from "../visible";
import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type AskReply, type Passage } from "../api";
import { editorLocation } from "../cite";
import { t, type Key } from "../i18n";

/** What the meaning index keeps on this computer, and a way to forget all of it. */
function MeaningIndex(props: { tick: number }) {
  const [info, { refetch }] = createResource(() => props.tick, () => api.docIndex().catch(() => null));
  const [confirming, setConfirming] = createSignal(false);
  const [done, setDone] = createSignal("");
  const [problem, setProblem] = createSignal("");
  const forget = async () => {
    setProblem("");
    try {
      const r = await api.forgetDocIndex();
      setConfirming(false);
      setDone(t("docs.index.done", { n: r.removed }));
      void refetch();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : String(e));
    }
  };
  return (
    <section class="card stack" aria-labelledby="h-index">
      <h2 id="h-index">{t("docs.index.title")}</h2>
      <p class="muted">{t("docs.index.lead")}</p>
      <Show when={info()} fallback={<LoadState loading={info.loading} retry={() => void refetch()} />}>
        {(i) => (
          <>
            <p>{i().vectors > 0 ? t("docs.index.count", { n: i().vectors, models: i().models.map((m) => m.model).join(", ") }) : t("docs.index.none")}</p>
            <Show when={i().vectors > 0 && !confirming()}>
              <div class="actions"><button type="button" onClick={() => { setDone(""); setConfirming(true); }}>{t("docs.index.forget")}</button></div>
            </Show>
            <Show when={confirming()}>
              <div role="alertdialog" aria-labelledby="index-confirm" class="stack">
                <p id="index-confirm"><strong>{t("docs.index.confirm", { n: i().vectors })}</strong></p>
                <div class="actions">
                  <button class="primary" type="button" onClick={() => void forget()}>{t("docs.index.yes")}</button>
                  <button type="button" onClick={() => setConfirming(false)}>{t("docs.index.no")}</button>
                </div>
              </div>
            </Show>
          </>
        )}
      </Show>
      <Show when={done()}><p role="status" class="ok">{done()}</p></Show>
      <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
    </section>
  );
}

export function Documents() {
  const [dir, setDir] = createSignal<string | undefined>(takeHint("kk-docs-folder") ?? undefined);
  const [listing, { refetch: refetchListing }] = createResource(() => dir() ?? "", (d) => api.fs(d || undefined).catch(() => null));
  const [question, setQuestion] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [asked, setAsked] = createSignal(0);
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
      setAsked(asked() + 1);
    }
  };
  const [passages, setPassages] = createSignal<Record<string, Passage | string>>({});
  const [copied, setCopied] = createSignal("");
  const copyLocation = async (loc: string) => {
    try {
      await navigator.clipboard.writeText(loc);
      setCopied(t("docs.copied", { where: loc }));
    } catch {
      setCopied(t("docs.copyFailed"));
    }
  };
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
        <p class="path" aria-live="polite">{chosen() || (listing.loading ? t("new.loading") : "")}</p>
        <div class="browser" role="group" aria-label={t("new.folders")}>
          <Show when={listing()} fallback={<LoadState loading={listing.loading} retry={() => void refetchListing()} />}>
            {(l) => (
              <>
                <Show when={l().parent}>{(p) => <button class="row" onClick={() => setDir(p())}>{t("new.up")}</button>}</Show>
                <For each={l().dirs} fallback={<p class="muted">{listing.loading ? t("new.loadingFolders") : t("new.noFolders")}</p>}>
                  {(d) => <button class="row" onClick={() => setDir(d.path)}>{d.name}</button>}
                </For>
              </>
            )}
          </Show>
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
            <Show when={r().checkable} fallback={r().no_answer ? <p class="warn">{t("docs.noAnswer")}</p> : <><p class="warn">{t("docs.unchecked")}</p><p class="mem-text">{r().unchecked_text}</p></>}>
              <p class="mem-text">{r().answer}</p>
              <p class="muted">{t("docs.notProof")}</p>
              <h3>{t("docs.checked")}</h3>
              <Show when={(r().citations ?? []).length > 0} fallback={<p class="muted">{t("docs.noQuotes")}</p>}>
                <p><strong>{t("docs.count", { v: r().verified ?? 0, n: (r().citations ?? []).length })}</strong></p>
                <Show when={copied()}><p role="status" class="ok">{copied()}</p></Show>
                <ul class="plain stack">
                  <For each={r().citations}>
                    {(c) => (
                      <li class="stack cite">
                        <p class="mem-text">&ldquo;{c.quote}&rdquo;</p>
                        <p class="muted"><code>{c.file}</code> <span class={`chip ${c.verdict === "verified" ? "ok" : "bad"}`}>{t(`docs.${c.verdict}` as Key, { line: c.line ?? 0 })}</span>
                          <Show when={c.verdict === "verified" && c.line}>
                            {" "}<button type="button" class="link" aria-expanded={!!passages()[`${c.file}:${c.line}`]} onClick={() => showPassage(c.file, c.line as number, chosen())}>{passages()[`${c.file}:${c.line}`] ? t("docs.hidePassage") : t("docs.showPassage")}</button>
                          </Show>
                          <Show when={c.verdict === "verified" && editorLocation(chosen(), c.file, c.line)}>
                            {(loc) => <>{" "}<button type="button" class="link" onClick={() => void copyLocation(loc())}>{t("docs.copyLocation")}</button></>}
                          </Show>
                        </p>
                        <Show when={passages()[`${c.file}:${c.line}`]}>
                          {(p) => (
                            <Show when={typeof p() !== "string"} fallback={<p role="alert" class="bad">{String(p())}</p>}>
                              <pre class="passage" tabindex="0" aria-label={t("docs.passageLabel", { file: c.file })}>
                                <For each={(p() as Passage).lines}>{(l, i) => <div class={(p() as Passage).start + i() === (p() as Passage).focus ? "hit" : ""}><span class="ln">{(p() as Passage).start + i()}</span> {visible(l)}</div>}</For>
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
            <Show when={r().narrowed_to}><p class="muted">{t("docs.narrowed", { n: r().narrowed_to ?? 0 })}</p></Show>
            <Show when={r().searched}><p class="muted">{t("docs.searched")} {t(r().by_meaning ? "docs.byMeaning" : "docs.byWords")}</p></Show>
            <Show when={(r().files_partial ?? []).length > 0}><p class="muted">{t("docs.partial", { list: list(r().files_partial ?? []) })}</p></Show>
            <Show when={r().files_skipped.length > 0}><p class="muted">{t(r().searched ? "docs.skippedSearched" : "docs.skipped", { list: list(r().files_skipped) })}</p></Show>
            <Show when={r().injection_suspects.length > 0}><p class="warn">{t("docs.injection", { list: list(r().injection_suspects) })}</p></Show>
          </section>
        )}
      </Show>
      <MeaningIndex tick={asked()} />
    </section>
  );
}
