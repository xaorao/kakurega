import { Lead } from "../components/Lead";
import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { ChosenFolder, shortPath } from "../components/ChosenFolder";
import { api, ApiError, type Question, type QuestionDepth, type QuestionPlace } from "../api";
import { LoadState } from "../components/LoadState";
import { t, type Key } from "../i18n";
import { visible } from "../visible";

const DEPTHS: QuestionDepth[] = ["quick", "normal", "thorough"];
const FOLDER_KEY = "kk-explore-folder";
const taken = (): string | undefined => {
  try {
    const v = sessionStorage.getItem(FOLDER_KEY) ?? undefined;
    sessionStorage.removeItem(FOLDER_KEY);
    return v || undefined;
  } catch {
    return undefined;
  }
};

/** What the program found about one place an answer rests on, in words. */
const placeWord = (p: QuestionPlace): string => {
  switch (p.found.state) {
    case "at_line": return t("ex.place.at_line");
    case "moved": return t("ex.place.moved", { line: p.found.line ?? 0 });
    case "not_found": return t("ex.place.not_found");
    case "too_short": return t("ex.place.too_short");
    case "withheld": return t("ex.place.withheld");
    default: return t("ex.place.no_such_file");
  }
};
const checked = (p: QuestionPlace) => p.found.state === "at_line" || p.found.state === "moved";

/** Ask an agent a question about a project: it reads the files on this computer, may run a short check in the sandbox, and
 *  every line its answer rests on is checked against the file. */
/** Says where the model that answered really is: only a model reached through Ollama is on this computer. */
export function footer(route: string, turns: number, elapsedMs: number): string {
  const at = route.indexOf(":");
  const kind = at < 0 ? "" : route.slice(0, at);
  const model = at < 0 ? route : route.slice(at + 1);
  const key = kind === "ollama" ? "ex.footer" : kind === "near" ? "ex.footer.near" : "ex.footer.other";
  return t(key, { turns, seconds: Math.max(1, Math.round(elapsedMs / 1000)), model });
}

export function Explore() {
  const [dir, setDir] = createSignal<string | undefined>(taken());
  const [listing, { refetch: refetchListing }] = createResource(() => dir() ?? "", (d) => api.fs(d || undefined).catch(() => null));
  const [earlier, { refetch }] = createResource(() => api.questionList().catch(() => null));
  const [question, setQuestion] = createSignal("");
  const [depth, setDepth] = createSignal<QuestionDepth>("normal");
  const [current, setCurrent] = createSignal<Question | null>(null);
  const [error, setError] = createSignal("");
  const [stopping, setStopping] = createSignal(false);
  const [madeRoom, setMadeRoom] = createSignal(0);
  const chosen = () => listing()?.path ?? "";
  let timer: number | undefined;
  const halt = () => {
    if (timer !== undefined) window.clearInterval(timer);
    timer = undefined;
  };
  onCleanup(halt);
  const follow = (id: string) => {
    halt();
    setStopping(false);
    const read = async () => {
      try {
        const v = await api.questionGet(id);
        setCurrent(v);
        if (v.state !== "running") {
          halt();
          void refetch();
        }
      } catch (err) {
        halt();
        setError(err instanceof ApiError ? err.message : t("ex.error"));
      }
    };
    void read();
    timer = window.setInterval(read, 1000);
  };
  const start = async (e: Event) => {
    e.preventDefault();
    setError("");
    setCurrent(null);
    try {
      const { id, forgotten } = await api.questionStart(chosen(), question().trim(), depth());
      setMadeRoom((forgotten ?? []).length);
      follow(id);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("ex.error"));
    }
  };
  const forget = async (id: string) => {
    setError("");
    try {
      await api.questionForget(id);
      if (current()?.id === id) setCurrent(null);
      void refetch();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("ex.error"));
    }
  };
  const running = () => current()?.state === "running";
  return (
    <section class="stack wide" aria-labelledby="h-ex">
      <h1 id="h-ex">{t("ex.title")}</h1>
      <Lead text={t("ex.lead")} tail={1} />
      <div class="card stack">
        <h2>{t("ex.step1")}</h2>
        <ChosenFolder path={chosen()} loading={listing.loading} />
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
      <form class="card stack" onSubmit={start}>
        <h2>{t("ex.step2")}</h2>
        <label for="ex-question">{t("ex.question")}</label>
        <textarea id="ex-question" rows={3} maxLength={2000} placeholder={t("ex.questionHint")} value={question()} onInput={(e) => setQuestion(e.currentTarget.value)} />
        <fieldset class="choices">
          <legend>{t("ex.depth")}</legend>
          <For each={DEPTHS}>
            {(d) => (
              <label class="row-check">
                <input type="radio" name="ex-depth" value={d} checked={depth() === d} onChange={() => setDepth(d)} />
                <span>{t(`ex.depth.${d}` as Key)} <span class="muted">{t(`ex.depth.${d}.hint` as Key)}</span></span>
              </label>
            )}
          </For>
        </fieldset>
        <div class="actions">
          <button class="primary" type="submit" disabled={running() || !chosen() || question().trim().length === 0}>{t("ex.ask")}</button>
          <Show when={running()}><span role="status" class="muted">{t("ex.working")}</span></Show>
          <Show when={madeRoom() > 0}><span role="status" class="muted">{t(madeRoom() === 1 ? "ex.madeRoom.one" : "ex.madeRoom", { n: madeRoom() })}</span></Show>
          <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
        </div>
      </form>
      <Show when={current()}>
        {(c) => (
          <section class="card stack" aria-labelledby="h-ex-answer" aria-busy={running()}>
            <h2 id="h-ex-answer">{visible(c().question)}</h2>
            <p class="muted">{c().project ?? shortPath(c().folder)} · {t(`ex.depth.${c().depth}` as Key)}</p>
            <Show when={c().state === "running"}>
              <ul class="plain stack" aria-live="polite"><For each={c().progress}>{(p) => <li class="muted">{p}</li>}</For></ul>
              <div class="actions">
                <button type="button" disabled={stopping()} onClick={async () => { try { await api.stopWork("question", c().id); setStopping(true); } catch (e) { setError(e instanceof ApiError ? e.message : t("work.stopError")); } }}>{t("work.stop")}</button>
                <Show when={stopping()}><span role="status" class="muted">{t("ex.stopping")}</span></Show>
              </div>
            </Show>
            <Show when={c().state === "failed"}><p role="alert" class="bad">{t("ex.failed")} {c().failure}</p></Show>
            <Show when={c().answer}>
              {(a) => (
                <>
                  <Show when={a().ending !== "answered"}><p class="warn" role="status">{t(`ex.ending.${a().ending}` as Key)}</p></Show>
                  <Show when={a().text}>
                    <p class="mem-text"><strong>{t("ex.saysModel")}:</strong> {visible(a().text)}</p>
                  </Show>
                  <Show when={a().places.length > 0} fallback={<Show when={a().ending === "answered" || a().ending === "unchecked"}><p class="warn">{t("ex.noPlaces")}</p></Show>}>
                    <p class={a().unchecked === 0 ? "ok" : "warn"}>{t("ex.checked", { n: a().places.length, found: a().checked })}</p>
                    <p class="muted">{t("ex.checkedMeans")}</p>
                    <ul class="plain stack" aria-label={t("ex.places")}>
                      <For each={a().places}>
                        {(p) => (
                          <li class="stack">
                            <p><code>{visible(p.file)}:{p.found.state === "moved" ? p.found.line : p.line}</code> <span class={checked(p) ? "chip ok" : "chip bad"}>{placeWord(p)}</span></p>
                            <Show when={checked(p)} fallback={<p class="muted">{t("ex.modelWrote")} <code>{visible(p.quote)}</code></p>}>
                              <pre class="passage" tabindex="0">{visible(p.text)}</pre>
                            </Show>
                          </li>
                        )}
                      </For>
                    </ul>
                  </Show>
                  <Show when={a().corrected}><p class="muted">{t("ex.corrected")}</p></Show>
                  <Show when={a().not_found}><p class="muted"><strong>{t("ex.notFound")}:</strong> {visible(a().not_found ?? "")}</p></Show>
                  <For each={a().checks}>
                    {(k, i) => (
                      <details>
                        <summary>{t("ex.check", { n: i() + 1, language: k.language === "javascript" ? "JavaScript" : "Python" })} · {t(!k.run.ran ? "ex.check.notRun" : k.run.timed_out ? "ex.check.timedOut" : k.run.exit_ok ? "ex.check.ok" : "ex.check.error")}</summary>
                        <p class="muted">{t("ex.check.what")}</p>
                        <pre class="passage" tabindex="0">{visible(k.script)}</pre>
                        <p class="muted">{t("ex.check.printed")}</p>
                        <pre class="passage" tabindex="0">{k.run.output ? visible(k.run.output) : t("ex.check.nothing")}</pre>
                      </details>
                    )}
                  </For>
                  <Show when={a().files_read.length > 0}>
                    <details>
                      <summary>{t(a().files_read.length === 1 ? "ex.read.one" : "ex.read", { n: a().files_read.length })}</summary>
                      <ul class="plain"><For each={a().files_read}>{(f) => <li><code>{visible(f)}</code></li>}</For></ul>
                    </details>
                  </Show>
                  <p class="muted">{footer(a().route, a().turns, a().elapsed_ms)}{a().withheld > 0 ? ` ${t(a().withheld === 1 ? "ex.withheld.one" : "ex.withheld", { n: a().withheld })}` : ""}</p>
                  <Show when={(a().withheld_files ?? []).length > 0}>
                    <details>
                      <summary>{t("ex.withheldWhich")}</summary>
                      <p class="muted">{t("ex.withheldWhy")}</p>
                      <ul class="plain"><For each={a().withheld_files}>{(f) => <li><code>{visible(f)}</code></li>}</For></ul>
                    </details>
                  </Show>
                </>
              )}
            </Show>
            <Show when={c().state !== "running"}>
              <div class="actions"><button type="button" class="linklike" onClick={() => void forget(c().id)}>{t("ex.forget")}</button></div>
            </Show>
          </section>
        )}
      </Show>
      <Show when={(earlier()?.questions ?? []).length > 0 || (!earlier() && !earlier.loading)}>
        <section class="card stack" aria-labelledby="h-ex-earlier">
          <h2 id="h-ex-earlier">{t("ex.earlier")}</h2>
          <Show when={!earlier()}><LoadState loading={false} retry={() => void refetch()} /></Show>
          <ul class="plain stack">
            <For each={earlier()?.questions}>
              {(x) => (
                <li>
                  <button type="button" class="link" onClick={() => follow(x.id)}>{visible(x.question)}</button>{" "}
                  <span class="muted">{x.project ?? ""}</span>{" "}
                  <span class="chip">{x.state === "answered" ? t("ex.checked.short", { n: (x.checked ?? 0) + (x.unchecked ?? 0), found: x.checked ?? 0 }) : t(`ex.state.${x.state}` as Key)}</span>
                </li>
              )}
            </For>
          </ul>
        </section>
      </Show>
    </section>
  );
}
