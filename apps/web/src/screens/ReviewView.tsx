import { Code } from "../components/Code";
import { createEffect, createMemo, createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { clockOffset, formatSpan, timeLeft } from "../expiry";
import { DiffView } from "../components/DiffView";
import { api, ApiError, type Config, type Fix, type Job } from "../api";
import { rich, t, type Key } from "../i18n";

type Tab = "progress" | "protected" | "source" | "findings" | "release";

const STAGES: [string, Key][] = [
  ["import", "stage.import"],
  ["protect", "stage.protect"],
  ["review", "stage.review"],
  ["check", "stage.check"],
  ["release", "stage.release"],
];

/** Names a finding's check language; a review stored before the field existed reads as Python, anything unknown is not repeated. */
function languageName(language?: string): string {
  if (language === undefined || language === "python") return t("find.lang.python");
  if (language === "javascript") return t("find.lang.javascript");
  if (language === "typescript") return t("find.lang.typescript");
  return t("find.lang.unknown");
}

function statusText(s: string): [string, string] {
  if (s === "reproduced") return [t("status.reproduced"), "ok"];
  if (s === "not_reproduced") return [t("status.not_reproduced"), "bad"];
  if (s === "nondeterministic") return [t("status.nondeterministic"), "bad"];
  if (s.startsWith("unavailable")) return [t("status.unavailable", { why: s.slice(13) }), "warn"];
  return [t("status.failed", { why: s.slice(8) }), "bad"];
}

function Progress(props: { job: Job }) {
  const reached = () => new Set(props.job.events.map((e) => e.stage));
  const finished = () => reached().has("finished");
  return (
    <ol class="steps" aria-label={t("progress.label")}>
      <For each={STAGES}>
        {([key, label]) => {
          const done = () => reached().has(key) && (finished() || STAGES.findIndex(([k]) => k === key) < STAGES.findIndex(([k]) => !reached().has(k)));
          const active = () => reached().has(key) && !done();
          return (
            <li class={done() ? "done" : active() ? "active" : ""}>
              <span class="mark" aria-hidden="true">{done() ? "✓" : active() ? "…" : ""}</span>
              <span>{t(label)}</span>
              <span class="sub">{props.job.events.filter((e) => e.stage === key).map((e) => e.detail).join(" ")}</span>
            </li>
          );
        }}
      </For>
      <Show when={props.job.state === "failed"}>
        <li class="fail"><span class="mark">!</span><span>{t("progress.stopped")}</span><span class="sub">{props.job.note}</span>
          <span class="actions"><button type="button" onClick={async () => { try { const r = await api.startJob(props.job.repo, props.job.recipient); location.hash = `#/job/${r.id}`; } catch { /* the folder may no longer be available; the old review stays as it was */ } }}>{t("progress.again")}</button></span>
        </li>
      </Show>
    </ol>
  );
}

function Protected(props: { job: Job }) {
  const [audit] = createResource(() => props.job.id, (id) => api.audit(id).then((r) => r.audit));
  const s = () => props.job.summary;
  return (
    <div class="stack">
      <Show when={s()} fallback={<p class="muted">{t("review.when")}</p>}>
        {(sum) => (
          <>
            <div class="card stack">
              <h2>{t("prot.cannotSee")}</h2>
              <Show when={sum().withheld.length > 0} fallback={<p class="muted">{t("prot.noSecrets")}</p>}>
                <ul class="plain">
                  <For each={sum().withheld}>{(w) => <li><code>{w.path}</code> <span class="chip bad">{t("prot.withheld")}</span> <span class="muted">{w.notes.join("; ")}</span></li>}</For>
                </ul>
              </Show>
              <Show when={sum().skipped.length > 0}>
                <p class="muted">{t("prot.skipped", { list: sum().skipped.map((x) => `${x.path} (${x.reason})`).join(", ") })}</p>
              </Show>
            </div>
            <div class="card stack">
              <h2>{t("prot.aiTitle")}</h2>
              <Show when={sum().injection_suspects.length > 0} fallback={<p class="muted">{t("prot.none")}</p>}>
                <p>{t("prot.aiBody")}</p>
                <ul class="plain"><For each={sum().injection_suspects}>{(p) => <li><code>{p}</code> <span class="chip warn">{t("prot.injection")}</span></li>}</For></ul>
              </Show>
            </div>
            <div class="card stack">
              <h2>{t("prot.dataWent")}</h2>
              <For each={sum().route}>{(r) => <p>{rich("prot.route", { model: r.model, route: r.class.replace("_", " "), endpoint: r.endpoint })}{r.simulated ? <span class="chip warn">{t("prot.simulated")}</span> : <span class="chip ok">{t("prot.real")}</span>}</p>}</For>
              <Show when={sum().final_taint}>{(t_) => <p>{rich("prot.taint", { level: t_().level, untrusted: t_().integrity === "untrusted" ? t("prot.untrusted") : "" })}</p>}</Show>
              <p class="muted">{rich("prot.manifest", { digest: sum().manifest_digest.slice(0, 16), count: sum().files_in_manifest })}</p>
            </div>
          </>
        )}
      </Show>
      <div class="card stack">
        <h2>{t("prot.requests")}</h2>
        <Show when={audit()} fallback={<p class="muted">{t("review.loading")}</p>}>
          {(a) => (
            <table>
              <thead><tr><th scope="col">#</th><th scope="col">{t("prot.col.op")}</th><th scope="col">{t("prot.col.result")}</th></tr></thead>
              <tbody>
                <For each={a()}>{(e) => <tr class={e.outcome === "ok" ? "" : "refused"}><td>{e.request}</td><td><code>{e.op}</code></td><td>{e.outcome === "ok" ? t("prot.allowed") : t("prot.refused", { why: e.outcome })}</td></tr>}</For>
              </tbody>
            </table>
          )}
        </Show>
      </div>
    </div>
  );
}

function Source(props: { job: Job }) {
  const [files] = createResource(() => props.job.id, (id) => api.files(id).then((r) => r.files));
  const [selected, setSelected] = createSignal<string | null>(null);
  const [content] = createResource(selected, (p) => (p ? api.file(props.job.id, p) : Promise.resolve(null)));
  return (
    <div class="split">
      <ul class="tree" aria-label={t("src.files")}>
        <For each={files()}>
          {(f) => (
            <li>
              <button class={selected() === f.path ? "row active" : "row"} onClick={() => setSelected(f.path)}>
                <span class="fname">{f.path}</span>
                <span class={`chip ${f.withheld ? "bad" : f.level === "internal" ? "" : "warn"}`}>{f.withheld ? t("prot.withheld") : f.level}</span>
              </button>
            </li>
          )}
        </For>
      </ul>
      <div class="viewer" aria-live="polite">
        <Show when={selected()} fallback={<p class="muted">{rich("src.choose")}</p>}>
          <Show when={content()}>
            {(c) => (
              <Show when={!c().withheld} fallback={<div class="redacted">{rich("src.withheld")}</div>}>
                <pre tabindex="0"><code>{c().text}</code></pre>
              </Show>
            )}
          </Show>
        </Show>
      </div>
    </div>
  );
}

export function Suggestions(props: { jobId: string; heading?: Key; lead?: Key; onChange?: () => void }) {
  const [items, { refetch }] = createResource(() => props.jobId, (id) => api.suggestions(id).then((r) => r.suggestions));
  const [note, setNote] = createSignal("");
  const act = async (fn: () => Promise<unknown>, msg = "") => {
    await fn().catch(() => undefined);
    setNote(msg);
    await refetch();
    props.onChange?.();
  };
  return (
    <Show when={(items() ?? []).length > 0}>
      <div class="card stack" role="region" aria-label={t(props.heading ?? "sug.heading")}>
        <h2>{t(props.heading ?? "sug.heading")}</h2>
        <p class="muted">{t(props.lead ?? "sug.lead")}</p>
        <ul class="plain stack">
          <For each={items()}>
            {(sg) => (
              <li class="stack">
                <p class="mem-text">{sg.text}</p>
                <p class="muted"><span class="chip warn">{t("mem.untrusted")}</span> {t("sug.why", { why: sg.why })}</p>
                <div class="actions">
                  <button class="primary" onClick={() => act(() => api.acceptSuggestion(props.jobId, sg.id), t("sug.accepted"))}>{t("sug.accept")}</button>
                  <button onClick={() => act(() => api.dismissSuggestion(props.jobId, sg.id))}>{t("sug.dismiss")}</button>
                </div>
              </li>
            )}
          </For>
        </ul>
        <Show when={note()}><p role="status" class="ok">{note()}</p></Show>
      </div>
    </Show>
  );
}

function Fixes(props: { jobId: string }) {
  const [items, { refetch }] = createResource(() => props.jobId, (id) => api.fixes(id).then((r) => r.fixes));
  const [asking, setAsking] = createSignal<number | null>(null);
  const [note, setNote] = createSignal("");
  const [err, setErr] = createSignal("");
  const undo = async (f: Fix) => {
    setErr("");
    try {
      const r = await api.undoFix(props.jobId, f.index, f.file);
      setNote(t(r.kept_later_edits ? "fix.undoneKept" : "fix.undone", { file: f.file }));
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
    await refetch();
  };
  const apply = async (f: Fix) => {
    setErr("");
    try {
      await api.applyFix(props.jobId, f.index, f.file);
      setNote(t("fix.applied", { file: f.file }));
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
    setAsking(null);
    await refetch();
  };
  return (
    <Show when={(items() ?? []).length > 0}>
      <div class="card stack" role="region" aria-label={t("fix.heading")}>
        <h2>{t("fix.heading")}</h2>
        <p class="muted">{t("fix.lead")}</p>
        <p class="muted">{t("fix.limit")}</p>
        <ul class="plain stack">
          <For each={items()}>
            {(f) => (
              <li class="stack">
                <p><code>{(f.files ?? [f.file]).join(", ")}</code> · {f.title}</p>
                <p>{f.explanation}</p>
                <details><summary>{t("fix.change")}</summary><DiffView diff={f.diff} label={t("fix.diffLabel", { file: f.file })} /></details>
                <p><span class={`chip ${f.verified ? "ok" : "bad"}`}>{f.verified ? t("fix.ok") : t("fix.notAccepted")}</span></p>
                <Show when={f.tests}><p class="muted wrap">{f.tests}</p></Show>
                <Show when={!f.verified}><p class="muted wrap">{t("fix.why", { why: f.status.replace(/^[a-z_]+: ?/, "").replace(/\s+/g, " ").slice(0, 240) })}</p></Show>
                <Show when={f.verified && !f.applied && f.state !== "attention" && f.state !== "applying"}>
                  <Show when={asking() === f.index} fallback={<div class="actions"><button onClick={() => setAsking(f.index)}>{t("fix.apply", { file: (f.files ?? [f.file]).length > 1 ? t("fix.nFiles", { n: (f.files ?? []).length }) : f.file })}</button></div>}>
                    <div class="stack" role="alertdialog" aria-label={t("fix.confirm", { file: (f.files ?? [f.file]).length > 1 ? (f.files ?? []).join(", ") : f.file })}>
                      <p>{t("fix.confirm", { file: (f.files ?? [f.file]).length > 1 ? (f.files ?? []).join(", ") : f.file })}</p>
                      <div class="actions">
                        <button class="primary" onClick={() => apply(f)}>{t("fix.yes")}</button>
                        <button onClick={() => setAsking(null)}>{t("fix.cancel")}</button>
                      </div>
                    </div>
                  </Show>
                </Show>
                <Show when={f.state === "attention"}><p role="alert" class="bad wrap">{t("fix.attention", { files: f.attention ?? "" })}</p></Show>
                <Show when={f.state === "applying"}><p role="status" class="muted">{t("fix.working")}</p></Show>
                <Show when={f.applied}><p><span class="chip ok">{t("fix.done")}</span> <button onClick={() => undo(f)}>{t("fix.undo")}</button></p></Show>
              </li>
            )}
          </For>
        </ul>
        <Show when={note()}><p role="status" class="ok">{note()}</p></Show>
        <Show when={err()}><p role="alert" class="bad">{err()}</p></Show>
      </div>
    </Show>
  );
}

function Findings(props: { job: Job }) {
  const [data] = createResource(() => props.job.id, (id) => api.findings(id));
  return (
    <div class="stack">
      <Suggestions jobId={props.job.id} />
      <Fixes jobId={props.job.id} />
      <Show when={data()} fallback={<p class="muted">{t("review.loading")}</p>}>
        {(d) => (
          <>
            <Show when={d().findings.length === 0}>
              <div class="card"><p>{t("find.none")}</p><p class="muted">{t("find.noneNote")}</p></div>
            </Show>
            <For each={d().findings}>
              {(f) => {
                const [statusLabel, cls] = statusText(f.status);
                return (
                  <article class="card stack">
                    <header class="between"><h2>{f.title}</h2><span class={`chip ${cls}`}>{statusLabel}</span></header>
                    <p><code>{f.file}:{f.line}</code></p>
                    <p>{f.explanation}</p>
                    <Show when={f.reproducer}>
                      <details><summary>{t("find.reproIn", { language: languageName(f.language) })}</summary><pre tabindex="0"><Code lang={f.language ?? "python"} text={f.reproducer} /></pre><Show when={f.output}><p class="muted">{t("find.output")}</p><pre tabindex="0"><code>{f.output}</code></pre></Show></details>
                    </Show>
                    <Show when={f.sample.length > 0}><p class="muted">{t("find.sample", { count: f.sample.length, files: f.sample.join(", ") })}</p></Show>
                  </article>
                );
              }}
            </For>
            <Show when={d().unparsed > 0}><p class="muted">{t("find.malformed", { count: d().unparsed })}</p></Show>
          </>
        )}
      </Show>
    </div>
  );
}

function ReleaseTab(props: { job: Job; onChange: () => void }) {
  const [release, { refetch }] = createResource(() => props.job.id, (id) => api.release(id));
  const [approval, setApproval] = createSignal<{ state: string; digest?: string; approval?: string }>({ state: "none" });
  const [how, setHow] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  // how long this release can still be approved: counted against the service's clock, once a second
  const [pageNow, setPageNow] = createSignal(Math.floor(Date.now() / 1000));
  const tick = setInterval(() => setPageNow(Math.floor(Date.now() / 1000)), 1000);
  onCleanup(() => clearInterval(tick));
  // the difference between the clocks is taken once, when the release arrives
  const offset = createMemo(() => clockOffset(release()?.server_time, Math.floor(Date.now() / 1000)));
  const left = createMemo(() => timeLeft(release()?.expires, pageNow() + offset()));
  const expired = () => left().state === "expired";

  // Only a different review restarts the watching; a refreshed copy of the same review must not.
  const jobId = createMemo(() => props.job.id);
  const poll = async (id = jobId()) => {
    try {
      setApproval(await api.approval(id));
    } catch {
      /* try again on the next tick */
    }
  };
  // The service pushes a notice whenever an approval changes, which App turns into this event, so nothing is asked every
  // second. The slow timer is only a safety net for a stream that is down.
  createEffect(() => {
    const id = jobId();
    void poll(id);
    const changed = () => void poll(id);
    window.addEventListener("kk-changed", changed);
    const t = setInterval(changed, 5000);
    onCleanup(() => {
      clearInterval(t);
      window.removeEventListener("kk-changed", changed);
    });
  });

  const ask = async () => {
    setBusy(true);
    setError("");
    try {
      const r = await api.requestApproval(props.job.id);
      setHow(r.how);
      await poll();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("appr.requestError"));
    } finally {
      setBusy(false);
    }
  };
  const deliver = async () => {
    setBusy(true);
    setError("");
    try {
      await api.deliver(props.job.id);
      props.onChange();
      await poll();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("appr.deliverError"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="stack">
      <Show when={release()} fallback={<p class="muted">{t("review.loading")}</p>}>
        {(r) => (
          <Show when={r().available} fallback={
            <div class="card stack">
              <h2>{t("rel.nothing")}</h2>
              <p>{t("rel.nothingBody")}</p>
              <For each={r().not_confirmed}>{(n) => <p class="muted">{n.title}: {n.why}</p>}</For>
            </div>
          }>
            <div class="card stack">
              <h2>{t("rel.exact")}</h2>
              <p>{rich("rel.for", { recipient: r().recipient ?? "", digest: r().digest ?? "" })}</p>
              <table>
                <thead><tr><th scope="col">{t("rel.col.file")}</th><th scope="col">{t("rel.col.size")}</th><th scope="col">SHA-256</th></tr></thead>
                <tbody><For each={r().files}>{(f) => <tr><td><code>{f.name}</code></td><td>{t("rel.bytes", { n: f.bytes })}</td><td><code>{f.sha256.slice(0, 16)}</code></td></tr>}</For></tbody>
              </table>
              <For each={r().files!.filter((f) => f.name.startsWith("finding-"))}>{(f) => <details open><summary>{f.name}</summary><pre tabindex="0"><code>{f.text}</code></pre></details>}</For>
              <h3>{t("rel.heldBack")}</h3>
              <ul class="plain"><For each={r().removed} fallback={<li class="muted">{t("rel.nothingHeld")}</li>}>{(x) => <li>{x.what}: <span class="muted">{x.why}</span></li>}</For></ul>
              <Show when={(r().personal_data ?? []).length > 0}>
                <h3>{t("rel.personal")}</h3>
                <p class="muted">{t("rel.personalLead")}</p>
                <ul class="plain"><For each={r().personal_data}>{(x) => <li><code>{x.file}</code>: {x.found}</li>}</For></ul>
              </Show>
              <Show when={r().not_confirmed.length > 0}>
                <h3>{t("rel.notConfirmed")}</h3>
                <ul class="plain"><For each={r().not_confirmed}>{(x) => <li>{x.title} <span class="muted">({x.why})</span></li>}</For></ul>
              </Show>
              <h3>{t("rel.notEstablished")}</h3>
              <ul class="plain"><For each={r().not_established}>{(x) => <li>{x}</li>}</For></ul>
            </div>
            <Show when={r().disclosure}>
              {(d) => (
                <div class="card stack" role="region" aria-label={t("disc.heading")}>
                  <h2>{t("disc.heading")}</h2>
                  <p>{t("disc.summary", { files: d().files_after, total: d().project_files, before: d().releases_before })}</p>
                  <p class={d().verdict === "allow" ? "ok" : d().verdict === "block" ? "bad" : "warn"}><strong>{t(`disc.${d().verdict}` as Key)}</strong></p>
                  <For each={d().reasons}>{(x) => <p class="warn">{x}</p>}</For>
                  <details>
                    <summary>{t("disc.checked")}</summary>
                    <ul class="plain"><For each={d().checked}>{(x) => <li>{x}</li>}</For></ul>
                    <p class="muted">{t("disc.notChecked")}</p>
                    <ul class="plain"><For each={d().not_checked}>{(x) => <li>{x}</li>}</For></ul>
                  </details>
                </div>
              )}
            </Show>
            <div class="card stack" aria-live="polite">
              <h2>{t("appr.title")}</h2>
              <Show when={!props.job.delivered && (approval().state === "none" || approval().state === "waiting" || approval().state === "approved")}>
                <Show when={left().state === "expired"}>
                  <p role="alert" class="bad">{t("appr.expired")}</p>
                </Show>
                <Show when={left().state === "soon"}>
                  <p class="warn" role="status">{t("appr.soon", { time: formatSpan((left() as { secs: number }).secs) })}</p>
                </Show>
                <Show when={left().state === "valid"}>
                  <p class="muted">{t("appr.validFor", { time: formatSpan((left() as { secs: number }).secs) })}</p>
                </Show>
              </Show>
              <Show when={approval().state === "none"}>
                <p>{t("appr.intro")}</p>
                <button class="primary" disabled={busy() || expired()} onClick={ask}>{t("appr.request")}</button>
              </Show>
              <Show when={approval().state === "waiting"}>
                <p>{rich("appr.waiting")}</p>
                <pre class="cmd" tabindex="0"><code>kaku approve {approval().approval}</code></pre>
                <p class="muted">{t("appr.typeDigest", { how: how() })}</p>
              </Show>
              <Show when={approval().state === "approved"}>
                <p class="ok">{t("appr.approved")}</p>
                <button class="primary" disabled={busy() || expired()} onClick={deliver}>{t("appr.deliver")}</button>
              </Show>
              <Show when={approval().state === "denied" || approval().state === "revoked"}><p class="bad">{t("appr.refused")}</p></Show>
              <Show when={approval().state === "uncertain"}><p class="warn">{t("appr.uncertain")}</p></Show>
              <Show when={props.job.delivered}>
                {(d) => (
                  <div class="stack">
                    <p class="ok">{rich("appr.delivered", { name: d().name, signer: d().signer.slice(0, 24) + "…" })}</p>
                    <p><a class="button" href={`/api/jobs/${props.job.id}/capsule`} download="review.kkg">{t("appr.download")}</a></p>
                    <p class="muted">{rich("appr.giveRecipient")}</p>
                  </div>
                )}
              </Show>
              <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
            </div>
          </Show>
        )}
      </Show>
    </div>
  );
}

export function ReviewView(props: { id: string; config: Config; onUpdate?: () => void }) {
  const [job, setJob] = createSignal<Job | null>(null);
  const [tab, setTab] = createSignal<Tab>("progress");
  const [auto, setAuto] = createSignal(true);
  const load = async () => {
    try {
      const j = await api.job(props.id);
      if (job()?.state !== j.state || job()?.delivered?.name !== j.delivered?.name) props.onUpdate?.();
      setJob(j);
      if (auto() && j.state !== "running") {
        setAuto(false);
        setTab(j.state === "ready" ? "release" : j.state === "nothing_to_release" ? "findings" : "progress");
      }
    } catch {
      /* ignore transient errors */
    }
  };
  createEffect(() => {
    props.id;
    setAuto(true);
    setTab("progress");
    setJob(null);
    void load();
    // The service pushes a notice whenever a review changes, which App turns into this event. The timer is only a safety
    // net for a stream that is down.
    const changed = () => void load();
    window.addEventListener("kk-changed", changed);
    const t = setInterval(load, 5000);
    onCleanup(() => {
      clearInterval(t);
      window.removeEventListener("kk-changed", changed);
    });
  });
  const tabs: [Tab, Key][] = [["progress", "tab.progress"], ["protected", "tab.protected"], ["source", "tab.source"], ["findings", "tab.findings"], ["release", "tab.release"]];
  return (
    <Show when={job()} fallback={<p class="muted">{t("review.loading")}</p>}>
      {(j) => (
        <section class="stack wide" aria-labelledby="h-job">
          <header class="between">
            <div>
              <h1 id="h-job">{j().repo.split("/").pop()}</h1>
              <p class="muted">{t("review.forRecipient", { recipient: j().recipient })}<span class={`state-text ${j().state}`}>{t(`state.${j().state}` as Key)}</span></p>
            </div>
          </header>
          <div
            class="tabs"
            role="tablist"
            onKeyDown={(e) => {
              const keys = tabs.map(([k]) => k);
              const i = keys.indexOf(tab());
              const next = e.key === "ArrowRight" ? (i + 1) % keys.length : e.key === "ArrowLeft" ? (i - 1 + keys.length) % keys.length : e.key === "Home" ? 0 : e.key === "End" ? keys.length - 1 : -1;
              if (next >= 0) {
                e.preventDefault();
                setTab(keys[next] as Tab);
                queueMicrotask(() => (e.currentTarget.querySelectorAll('[role="tab"]')[next] as HTMLElement | undefined)?.focus());
              }
            }}
          >
            <For each={tabs}>{([k, label]) => <button role="tab" tabindex={tab() === k ? 0 : -1} aria-selected={tab() === k} class={tab() === k ? "tab active" : "tab"} onClick={() => setTab(k)}>{t(label)}</button>}</For>
          </div>
          <div role="tabpanel">
            <Show when={tab() === "progress"}><Progress job={j()} /></Show>
            <Show when={tab() === "protected"}><Protected job={j()} /></Show>
            <Show when={tab() === "source"}><Show when={j().summary} fallback={<p class="muted">{t("review.when")}</p>}><Source job={j()} /></Show></Show>
            <Show when={tab() === "findings"}><Show when={j().summary} fallback={<p class="muted">{t("review.when")}</p>}><Findings job={j()} /></Show></Show>
            <Show when={tab() === "release"}><Show when={j().summary} fallback={<p class="muted">{t("review.when")}</p>}><ReleaseTab job={j()} onChange={load} /></Show></Show>
          </div>
        </section>
      )}
    </Show>
  );
}
