import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { ChosenFolder } from "../components/ChosenFolder";
import { api, ApiError, type CodeTask as Task } from "../api";
import { DiffView } from "../components/DiffView";
import { LoadState } from "../components/LoadState";
import { t, type Key } from "../i18n";
import { visible } from "../visible";
import { McpPending } from "./Mcp";

const STATES = ["running", "ready", "failed", "applied", "applying", "undoing", "attention"];
const stateWord = (s: string) => (STATES.includes(s) ? t(`code.state.${s}` as Key) : s);
const done = (s: Task["state"]) => s !== "running" && s !== "applying" && s !== "undoing";

const STEP_ROWS = 60;
const KNOWN_STEPS = ["list_files", "read_file", "search_text", "edit_file", "create_file", "delete_file", "run_tests", "finish"];
const stepWord = (tool: string) => KNOWN_STEPS.includes(tool) ? t(`code.step.${tool}` as Key) : t("code.step.other", { tool: visible(tool).slice(0, 40) });

export function CodeTask() {
  const [dir, setDir] = createSignal<string | undefined>(undefined);
  const [listing, { refetch: refetchListing }] = createResource(() => dir() ?? "", (d) => api.fs(d || undefined).catch(() => null));
  // a list that could not be read is said so, never shown as "no earlier tasks"
  const [earlier, { refetch }] = createResource(() => api.codeList().catch(() => null));
  const [task, setTask] = createSignal("");
  const [current, setCurrent] = createSignal<Task | null>(null);
  const [error, setError] = createSignal("");
  const [stopping, setStopping] = createSignal(false);
  const [note, setNote] = createSignal("");
  const [confirming, setConfirming] = createSignal(false);
  const [acting, setActing] = createSignal(false);
  const chosen = () => listing()?.path ?? "";
  let timer: number | undefined;
  const stop = () => {
    if (timer !== undefined) window.clearInterval(timer);
    timer = undefined;
  };
  onCleanup(stop);
  const follow = (id: string) => {
    stop();
    const read = async () => {
      try {
        const v = await api.codeGet(id);
        setCurrent(v);
        if (done(v.state)) {
          stop();
          void refetch();
        }
      } catch (err) {
        stop();
        setError(err instanceof ApiError ? err.message : t("code.error"));
      }
    };
    void read();
    timer = window.setInterval(read, 1000);
  };
  const start = async (e: Event) => {
    e.preventDefault();
    setError("");
    setNote("");
    setConfirming(false);
    setCurrent(null);
    setStopping(false);
    try {
      const { id } = await api.codeStart(chosen(), task().trim());
      follow(id);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("code.error"));
    }
  };
  const act = async (which: "apply" | "undo") => {
    const c = current();
    if (!c) return;
    setActing(true);
    setError("");
    try {
      if (which === "apply") {
        await api.codeApply(c.id, c.digest ?? "");
        setNote(t("code.applied"));
      } else {
        await api.codeUndo(c.id);
        setNote(t("code.undone"));
      }
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("code.error"));
    } finally {
      setConfirming(false);
      setActing(false);
      setCurrent(await api.codeGet(c.id).catch(() => c));
      void refetch();
    }
  };
  const running = () => current() !== null && !done((current() as Task).state);
  return (
    <section class="stack wide" aria-labelledby="h-code">
      <h1 id="h-code">{t("code.title")}</h1>
      <McpPending />
      <p class="lead">{t("code.lead")}</p>
      <div class="card stack">
        <h2>{t("code.step1")}</h2>
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
        <h2>{t("code.step2")}</h2>
        <label for="code-task">{t("code.task")}</label>
        <textarea id="code-task" rows={3} value={task()} onInput={(e) => setTask(e.currentTarget.value)} maxLength={2000} />
        <div class="actions">
          <button class="primary" type="submit" disabled={running() || !chosen() || task().trim().length === 0}>{t("code.start")}</button>
          <Show when={running()}><span role="status" class="muted">{t("code.working")}</span></Show>
          <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
        </div>
      </form>
      <Show when={current()}>
        {(c) => (
          <section class="card stack" aria-labelledby="h-code-result" aria-busy={running()}>
            <h2 id="h-code-result">{t("code.result")} <span class="muted">{c().id}</span></h2>
            <ul class="plain stack" aria-live="polite">
              <For each={c().progress}>{(p) => <li class="muted">{p}</li>}</For>
            </ul>
            <Show when={c().state === "running"}>
              <div class="actions">
                <button type="button" disabled={stopping()} onClick={async () => { try { await api.stopWork("code", c().id); setStopping(true); } catch (e) { setError(e instanceof ApiError ? e.message : t("work.stopError")); } }}>{t("work.stop")}</button>
                <Show when={stopping()}><span role="status" class="muted">{t("work.stoppingCode")}</span></Show>
              </div>
            </Show>
            <Show when={c().state === "failed"}><p role="alert" class="bad">{t("code.failed")} {c().error}</p></Show>
            <Show when={c().state === "attention"}><p role="alert" class="bad">{t("code.attention", { list: c().attention.join(", ") })}</p></Show>
            <Show when={done(c().state) && c().state !== "failed"}>
              <Show when={c().ended}><p>{c().ended}</p></Show>
              <Show when={(c().steps ?? []).length > 0}>
                <details class="steps">
                  <summary>{t("code.steps", { n: c().steps.length })}</summary>
                  <ol class="plain stack">
                    <For each={c().steps.slice(0, STEP_ROWS)}>
                      {(s) => (
                        <li class={s.refused ? "warn" : "muted"}>
                          {stepWord(s.tool)}{s.label ? <> <code>{s.label}</code></> : ""}
                          <Show when={s.refused}> <span class="chip">{t("code.step.refused")}</span> {s.said}</Show>
                        </li>
                      )}
                    </For>
                    <Show when={c().steps.length > STEP_ROWS}><li class="muted">{t("code.step.more", { n: c().steps.length - STEP_ROWS })}</li></Show>
                  </ol>
                </details>
              </Show>
              <Show when={c().summary}><p class="mem-text"><strong>{t("code.saysModel")}:</strong> {c().summary}</p></Show>
              <Show when={(c().withheld_files ?? 0) > 0}><p class="muted">{t("code.withheld", { n: c().withheld_files ?? 0 })}</p></Show>
              <Show when={c().tests} fallback={<p class="warn">{t("code.testsNone")}</p>}>
                {(tr) => (
                  <>
                    <p class={tr().ran === false ? "warn" : tr().passed ? "ok" : "bad"}>{t(tr().ran === false ? "code.testsNotRun" : tr().passed ? "code.testsPass" : "code.testsFail", { cmd: c().test_command ?? "" })}</p>
                    <details><summary>{t("code.testOutput")}</summary><pre class="passage" tabindex="0">{tr().output}</pre></details>
                  </>
                )}
              </Show>
              <For each={c().concerns}>{(x) => <p class="warn">{x}</p>}</For>
              <Show when={c().changes.length > 0} fallback={<p class="muted">{t("code.noChange")}</p>}>
                <For each={c().changes}>
                  {(ch) => (
                    <div class="stack">
                      <p class="code-file"><code>{ch.path}</code> <span class="chip">{t(`code.${ch.kind}` as Key)}</span> <span class="muted">+{ch.added} −{ch.removed}</span></p>
                      <DiffView diff={ch.diff} label={t("code.diffLabel", { file: ch.path })} />
                    </div>
                  )}
                </For>
                <p class="muted">{t("code.digest")}: <code>{(c().digest ?? "").slice(0, 16)}</code></p>
              </Show>
              <Show when={note()}><p role="status" class="ok">{note()}</p></Show>
              <div class="actions">
                <Show when={c().state === "ready" && c().can_apply && !confirming()}>
                  <button class="primary" type="button" onClick={() => setConfirming(true)}>{t("code.apply")}</button>
                </Show>
                <Show when={c().state === "ready" && !c().can_apply && c().changes.length > 0}><p class="bad">{t("code.cannot")}</p></Show>
                <Show when={confirming()}>
                  <div role="alertdialog" aria-labelledby="code-confirm" class="stack">
                    <p id="code-confirm"><strong>{t("code.confirm", { folder: c().folder })}</strong></p>
                    <div class="actions">
                      <button class="primary" type="button" disabled={acting()} onClick={() => void act("apply")}>{t("code.confirmYes")}</button>
                      <button type="button" onClick={() => setConfirming(false)}>{t("code.cancel")}</button>
                    </div>
                  </div>
                </Show>
                <Show when={c().state === "applied"}>
                  <button type="button" disabled={acting()} onClick={() => void act("undo")}>{t("code.undo")}</button>
                </Show>
              </div>
            </Show>
          </section>
        )}
      </Show>
      <Show when={(earlier()?.tasks ?? []).length > 0 || (!earlier() && !earlier.loading)}>
        <section class="card stack" aria-labelledby="h-code-earlier">
          <h2 id="h-code-earlier">{t("code.earlier")}</h2>
          <Show when={!earlier()}><LoadState loading={false} retry={() => void refetch()} /></Show>
          <ul class="plain stack">
            <For each={earlier()?.tasks}>
              {(x) => (
                <li>
                  <button type="button" class="link" onClick={() => { setNote(""); setConfirming(false); follow(x.id); }}>{t("code.open")}</button>{" "}
                  <code>{x.id}</code> <span class="chip">{stateWord(x.state)}</span> {x.task}
                </li>
              )}
            </For>
          </ul>
        </section>
      </Show>
    </section>
  );
}
