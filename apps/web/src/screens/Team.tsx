import { createEffect, createMemo, createResource, createSignal, For, on, onCleanup, Show, untrack } from "solid-js";
import { api, ApiError, type AttachmentSummary, type ExpertInfo, type TeamRun, type TeamTask } from "../api";
import { MAX_BYTES, MAX_FILES, sizeText } from "../attach";
import { LoadState } from "../components/LoadState";
import { Markdown } from "../components/Markdown";
import { t } from "../i18n";

/** A goal handed to a lead: the plan it wrote, each task as it runs, the questions it has for the owner, and the result. */
export function Team(props: { id?: string }) {
  // a list that could not be read is said so with a way to try again, never shown as "no experts" or "no runs"
  const [roster, { refetch: refetchRoster }] = createResource(() => api.experts().catch(() => null));
  const everyone = createMemo<ExpertInfo[]>(() => [...(roster()?.experts ?? []), ...(roster()?.yours ?? []).map((y) => ({ ...y, group: t("rt.yours") })), ...(roster()?.workers ?? []).map((y) => ({ ...y, group: t("team.workers") })), ...(roster()?.teammates ?? []).map((y) => ({ ...y, group: t("team.outside") }))]);
  const [runs, { refetch: refetchRuns }] = createResource(() => api.teamRuns().catch(() => null));
  const [goal, setGoal] = createSignal("");
  const [allowed, setAllowed] = createSignal<string[]>([]);
  const [run, setRun] = createSignal<TeamRun | null>(null);
  const [planning, setPlanning] = createSignal(false);
  const [error, setError] = createSignal("");
  const [answers, setAnswers] = createSignal<Record<number, string>>({});
  const [confirmDelete, setConfirmDelete] = createSignal(false);
  const [acting, setActing] = createSignal(false);

  const groups = createMemo(() => {
    const out: { group: string; experts: ExpertInfo[] }[] = [];
    for (const e of everyone()) {
      const g = out.find((x) => x.group === e.group);
      if (g) g.experts.push(e);
      else out.push({ group: e.group, experts: [e] });
    }
    return out;
  });

  const fail = (err: unknown) => setError(err instanceof ApiError ? err.message : t("team.error"));
  const moving = () => run()?.status === "running" || run()?.working === true;

  // while a run is working, ask for its state once a second; a page left open on a finished run asks nothing
  const timer = setInterval(() => {
    const r = run();
    if (!r || !moving()) return;
    api.teamRun(r.id).then(
      (next) => {
        if (run()?.id !== next.id) return;
        const ended = !(next.status === "running" || next.working);
        setRun(next);
        if (ended) void refetchRuns();
      },
      () => undefined,
    );
  }, 1000);
  onCleanup(() => clearInterval(timer));

  const act = async (what: () => Promise<unknown>) => {
    if (acting()) return;
    setError("");
    setActing(true);
    try {
      await what();
    } catch (err) {
      fail(err);
    } finally {
      setActing(false);
    }
  };

  // the project the run is about: its notes are offered to the experts, and its folder is what the project reviewer reads
  const [projects] = createResource(() => api.projects().then((r) => r.projects.filter((p) => p.listed), () => []));
  const [project, setProject] = createSignal("");
  // files for the whole run: read on this computer like a chat's files, and given to every expert as checked data
  const [files, setFiles] = createSignal<AttachmentSummary[]>([]);
  const [fileNote, setFileNote] = createSignal("");
  let picker: HTMLInputElement | undefined;
  const addFiles = async (list: FileList | null) => {
    setFileNote("");
    for (const f of Array.from(list ?? [])) {
      if (files().length >= MAX_FILES) { setFileNote(t("team.filesMax", { n: MAX_FILES })); break; }
      if (f.size > MAX_BYTES) { setFileNote(t("files.tooBig", { name: f.name, max: sizeText(MAX_BYTES) })); continue; }
      try {
        const a = await api.attachFile(f);
        setFiles((x) => [...x, a]);
      } catch (err) {
        setFileNote(err instanceof ApiError ? err.message : t("team.filesError", { name: f.name }));
      }
    }
  };
  const dropFile = (id: string) => {
    setFiles((x) => x.filter((f) => f.id !== id));
    void api.removeAttachment(id).catch(() => { /* it expires by itself */ });
  };

  const plan = (e: Event) => {
    e.preventDefault();
    if (!goal().trim() || planning()) return;
    setError("");
    setPlanning(true);
    setConfirmDelete(false);
    api.teamPlan(goal().trim(), allowed(), files().map((f) => f.id), project()).then(
      (r) => {
        show(r);
        setAnswers({});
        setGoal("");
        setFiles([]);
        void refetchRuns();
      },
      fail,
    ).finally(() => setPlanning(false));
  };

  const show = (r: TeamRun | null) => {
    setRun(r);
    const want = r ? `#/team/${r.id}` : "#/team";
    if (location.hash !== want) location.hash = want;
  };
  const open = (id: string) => act(async () => {
    show(await api.teamRun(id));
    setAnswers({});
    setConfirmDelete(false);
  });
  // a link to one run (from "Waiting for you", or a page reload) opens that run
  // It follows the address only: showing a run changes the address, and that must not ask for the run that was shown before.
  createEffect(on(() => props.id ?? "", (id) => {
    if (/^team-[0-9a-f]{12}$/.test(id) && untrack(run)?.id !== id) void open(id);
  }));
  const start = () => act(async () => setRun(await api.teamStart(run()!.id)));
  const stop = () => act(async () => {
    await api.teamStop(run()!.id);
    setRun(await api.teamRun(run()!.id));
    void refetchRuns();
  });
  const remove = () => act(async () => {
    await api.teamDelete(run()!.id);
    show(null);
    setConfirmDelete(false);
    void refetchRuns();
  });
  const answer = (task: TeamTask) => act(async () => {
    const text = (answers()[task.n] ?? "").trim();
    if (!text) return;
    setRun(await api.teamAnswer(run()!.id, task.n, text));
    setAnswers((a) => ({ ...a, [task.n]: "" }));
  });

  const stateChip = (s: TeamTask["state"]) => (s === "done" ? "chip ok" : s === "failed" ? "chip bad" : s === "needs_input" ? "chip warn" : "chip");
  const statusChip = (s: string) => (s === "done" ? "chip ok" : s === "waiting" || s === "interrupted" ? "chip warn" : "chip");
  const canStart = () => ["planned", "stopped", "interrupted", "waiting"].includes(run()?.status ?? "") && !run()?.working;
  const startLabel = () => (run()?.status === "planned" ? t("team.start") : t("team.carryOn"));
  const hasQuestion = () => (run()?.tasks ?? []).some((x) => x.state === "needs_input");

  return (
    <section class="stack" aria-labelledby="team-title">
      <h1 id="team-title">{t("team.title")}</h1>
      <p class="muted">{t("team.lead")}</p>
      <form class="card stack" onSubmit={plan}>
        <label for="team-goal">{t("team.goal")}</label>
        <textarea id="team-goal" rows="3" maxlength="4000" value={goal()} disabled={planning()} onInput={(e) => setGoal(e.currentTarget.value)} />
        <Show when={(projects() ?? []).length > 0}>
          <label for="team-project">{t("team.project")}</label>
          <select id="team-project" value={project()} disabled={planning()} onChange={(e) => setProject(e.currentTarget.value)}>
            <option value="">{t("team.projectNone")}</option>
            <For each={projects() ?? []}>{(p) => <option value={p.name}>{p.folder_available ? t("team.projectWithFolder", { name: p.name }) : p.name}</option>}</For>
          </select>
          <p class="muted">{t("team.projectHelp")}</p>
        </Show>
        <div class="stack team-files">
          <div class="actions">
            <input ref={picker} id="team-files" type="file" multiple hidden aria-label={t("files.attach")} onChange={(e) => { void addFiles(e.currentTarget.files); e.currentTarget.value = ""; }} />
            <button type="button" disabled={planning()} onClick={() => picker?.click()}>{t("files.attach")}</button>
            <span class="muted">{t("team.filesHelp")}</span>
          </div>
          <Show when={files().length > 0}>
            <ul class="plain stack" aria-label={t("team.filesList")}>
              <For each={files()}>
                {(f) => (
                  <li>
                    <strong>{f.name}</strong> <span class="muted">{sizeText(f.bytes)} · {t("files.chars", { n: f.chars })}{f.flagged.secret > 0 ? ` · ${t("team.filesWithheld", { n: f.flagged.secret })}` : ""}</span>{" "}
                    <button type="button" class="link" onClick={() => dropFile(f.id)}>{t("files.remove")}</button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
          <Show when={fileNote()}><span role="status" class="muted">{fileNote()}</span></Show>
        </div>
        <details class="team-allow">
          <summary>{t("team.allow")}</summary>
          <fieldset class="rt-experts">
            <legend class="muted">{t("team.allowHelp")}</legend>
            <For each={groups()}>
              {(g) => (
                <div class="rt-group">
                  <h2>{g.group}</h2>
                  <For each={g.experts}>
                    {(x) => (
                      <label class="rt-expert">
                        <input type="checkbox" checked={allowed().includes(x.id)} disabled={planning()} onChange={() => setAllowed((a) => (a.includes(x.id) ? a.filter((y) => y !== x.id) : [...a, x.id]))} />
                        <span><strong>{x.name}</strong><span class="muted"> {x.summary}</span></span>
                      </label>
                    )}
                  </For>
                </div>
              )}
            </For>
          </fieldset>
        </details>
        <Show when={!roster() && !roster.loading}><LoadState loading={false} retry={() => void refetchRoster()} /></Show>
        <div class="rt-actions">
          <button class="primary" type="submit" disabled={!goal().trim() || planning()}>{planning() ? t("team.planning") : t("team.plan")}</button>
          <span class="muted">{t("team.planNote")}</span>
        </div>
      </form>
      <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>

      <Show when={run()}>
        {(r) => (
          <section class="stack" aria-labelledby="team-run">
            <div class="card stack">
              <h2 id="team-run">{t("team.run")} <span class={statusChip(r().status)} data-status={r().status}>{t(`team.status.${r().status}`)}</span></h2>
              <p class="team-goal">{r().goal}</p>
              <Show when={(r().files ?? []).length > 0}>
                <p class="muted">{t("team.filesWith", { list: (r().files ?? []).map((n) => n ?? t("team.fileGone")).join(", ") })}</p>
              </Show>
              <p class="muted" role="status" aria-live="polite">{t(`team.help.${r().status}`)}</p>
              <div class="rt-actions">
                <Show when={canStart() && !(r().status === "waiting" && hasQuestion())}><button class="primary" type="button" disabled={acting()} onClick={start}>{startLabel()}</button></Show>
                <Show when={moving()}><button type="button" disabled={acting()} onClick={stop}>{t("team.stop")}</button></Show>
                <Show when={!moving() && !confirmDelete()}><button type="button" onClick={() => setConfirmDelete(true)}>{t("team.delete")}</button></Show>
                <Show when={confirmDelete()}>
                  <span>{t("team.deleteAsk")}</span>
                  <button type="button" class="danger" disabled={acting()} onClick={remove}>{t("team.deleteYes")}</button>
                  <button type="button" onClick={() => setConfirmDelete(false)}>{t("team.deleteNo")}</button>
                </Show>
              </div>
            </div>

            <Show when={r().final || r().final_note}>
              <section class="card stack rt-resolution" aria-labelledby="team-final">
                <h2 id="team-final">{t("team.result")}</h2>
                <Show when={r().final} fallback={<p role="status">{r().final_note}</p>}>
                  <div class="rt-decision"><Markdown text={r().final ?? ""} /></div>
                </Show>
                <p class="muted">{t("team.resultNote")}</p>
              </section>
            </Show>

            <h2>{t("team.tasks")}</h2>
            <ol class="plain stack team-tasks">
              <For each={r().tasks}>
                {(task) => (
                  <li class="card stack" data-task={task.n} data-state={task.state}>
                    <p>
                      <strong>{t("team.task", { n: task.n })}</strong> · {task.expert_name} <span class={stateChip(task.state)}>{t(`team.state.${task.state}`)}</span>
                      <Show when={task.after.length > 0}><span class="muted"> {t("team.after", { list: task.after.join(", ") })}</span></Show>
                    </p>
                    <p>{task.task}</p>
                    <For each={task.asked}>
                      {(qa) => <p class="muted team-qa"><strong>{t("team.asked")}</strong> {qa.question}<br /><strong>{t("team.answered")}</strong> {qa.answer}</p>}
                    </For>
                    <Show when={task.state === "needs_input"}>
                      <form class="stack team-ask" onSubmit={(e) => { e.preventDefault(); void answer(task); }}>
                        <label for={`team-answer-${task.n}`}><strong>{t("team.question")}</strong> {task.question}</label>
                        <textarea id={`team-answer-${task.n}`} rows="2" maxlength="2000" value={answers()[task.n] ?? ""} onInput={(e) => setAnswers((a) => ({ ...a, [task.n]: e.currentTarget.value }))} />
                        <div class="rt-actions"><button class="primary" type="submit" disabled={acting() || !(answers()[task.n] ?? "").trim()}>{t("team.answer")}</button></div>
                      </form>
                    </Show>
                    <Show when={task.result}><Markdown text={task.result} /></Show>
                    <Show when={task.note}><p class="muted">{t(task.state === "done" ? "team.note" : "team.why")} {task.note}.</p></Show>
                  </li>
                )}
              </For>
            </ol>
          </section>
        )}
      </Show>

      <h2>{t("team.earlier")}</h2>
      <Show when={(runs()?.runs ?? []).length > 0} fallback={<Show when={runs()} fallback={<LoadState loading={runs.loading} retry={() => void refetchRuns()} />}><p class="muted">{t("team.none")}</p></Show>}>
        <ul class="plain stack team-runs">
          <For each={runs()?.runs ?? []}>
            {(x) => (
              <li>
                <button type="button" class="row" aria-current={run()?.id === x.id ? "true" : undefined} onClick={() => void open(x.id)}>
                  <span class="team-run-goal">{x.goal}</span>
                  <span class="muted"> <span class={statusChip(x.status)}>{t(`team.status.${x.status}`)}</span> {t("team.counts", { done: x.done, tasks: x.tasks })}<Show when={x.needs_input > 0}> · {t("team.waitingYou", { n: x.needs_input })}</Show></span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </section>
  );
}
