import { createEffect, createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api, ApiError, type Config, type Job } from "../api";

type Tab = "progress" | "protected" | "source" | "findings" | "release";

const STAGES: [string, string][] = [
  ["import", "Read the project"],
  ["protect", "Label files and withhold secrets"],
  ["review", "Agent reviews in the sandbox"],
  ["check", "Kernel re-runs each reproducer"],
  ["release", "Prepare the release"],
];

function statusText(s: string): [string, string] {
  if (s === "reproduced") return ["Confirmed by the kernel", "ok"];
  if (s === "not_reproduced") return ["Not reproduced", "bad"];
  if (s === "nondeterministic") return ["Result changed between runs", "bad"];
  if (s.startsWith("unavailable")) return ["Could not be checked: " + s.slice(13), "warn"];
  return ["Check failed: " + s.slice(8), "bad"];
}

function Progress(props: { job: Job }) {
  const reached = () => new Set(props.job.events.map((e) => e.stage));
  const finished = () => reached().has("finished");
  return (
    <ol class="steps" aria-label="Progress">
      <For each={STAGES}>
        {([key, label]) => {
          const done = () => reached().has(key) && (finished() || STAGES.findIndex(([k]) => k === key) < STAGES.findIndex(([k]) => !reached().has(k)));
          const active = () => reached().has(key) && !done();
          return (
            <li class={done() ? "done" : active() ? "active" : ""}>
              <span class="mark" aria-hidden="true">{done() ? "✓" : active() ? "…" : ""}</span>
              <span>{label}</span>
              <span class="sub">{props.job.events.filter((e) => e.stage === key).map((e) => e.detail).join(" ")}</span>
            </li>
          );
        }}
      </For>
      <Show when={props.job.state === "failed"}>
        <li class="fail"><span class="mark">!</span><span>The review stopped</span><span class="sub">{props.job.note}</span></li>
      </Show>
    </ol>
  );
}

function Protected(props: { job: Job }) {
  const [audit] = createResource(() => props.job.id, (id) => api.audit(id).then((r) => r.audit));
  const s = () => props.job.summary;
  return (
    <div class="stack">
      <Show when={s()} fallback={<p class="muted">Available when the review finishes.</p>}>
        {(sum) => (
          <>
            <div class="card stack">
              <h2>What the agent could not see</h2>
              <Show when={sum().withheld.length > 0} fallback={<p class="muted">No secret files were found.</p>}>
                <ul class="plain">
                  <For each={sum().withheld}>{(w) => <li><code>{w.path}</code> <span class="chip bad">withheld</span> <span class="muted">{w.notes.join("; ")}</span></li>}</For>
                </ul>
              </Show>
              <Show when={sum().skipped.length > 0}>
                <p class="muted">Skipped: {sum().skipped.map((x) => `${x.path} (${x.reason})`).join(", ")}</p>
              </Show>
            </div>
            <div class="card stack">
              <h2>Text addressed to AI tools</h2>
              <Show when={sum().injection_suspects.length > 0} fallback={<p class="muted">None found.</p>}>
                <p>These files contain instructions aimed at AI assistants. They were treated as data and nothing in them was followed.</p>
                <ul class="plain"><For each={sum().injection_suspects}>{(p) => <li><code>{p}</code> <span class="chip warn">possible prompt injection</span></li>}</For></ul>
              </Show>
            </div>
            <div class="card stack">
              <h2>Where the data went</h2>
              <For each={sum().route}>{(r) => <p>Model <strong>{r.model}</strong> on route <strong>{r.class.replace("_", " ")}</strong> at <code>{r.endpoint}</code>{r.simulated ? <span class="chip warn">simulated</span> : <span class="chip ok">real model</span>}</p>}</For>
              <Show when={sum().final_taint}>{(t) => <p>Everything the agent produced is labelled <strong>{t().level}</strong>{t().integrity === "untrusted" ? " and untrusted (it came from content that could contain instructions)" : ""}.</p>}</Show>
              <p class="muted">Source manifest digest <code>{sum().manifest_digest.slice(0, 16)}</code> over {sum().files_in_manifest} files. Only a keyed commitment to it is signed into the capsule.</p>
            </div>
          </>
        )}
      </Show>
      <div class="card stack">
        <h2>Every request the agent made</h2>
        <Show when={audit()} fallback={<p class="muted">Loading…</p>}>
          {(a) => (
            <table>
              <thead><tr><th>#</th><th>Operation</th><th>Result</th></tr></thead>
              <tbody>
                <For each={a()}>{(e) => <tr class={e.outcome === "ok" ? "" : "refused"}><td>{e.request}</td><td><code>{e.op}</code></td><td>{e.outcome === "ok" ? "allowed" : `refused: ${e.outcome}`}</td></tr>}</For>
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
      <ul class="tree" aria-label="Files">
        <For each={files()}>
          {(f) => (
            <li>
              <button class={selected() === f.path ? "row active" : "row"} onClick={() => setSelected(f.path)}>
                <span class="fname">{f.path}</span>
                <span class={`chip ${f.withheld ? "bad" : f.level === "internal" ? "" : "warn"}`}>{f.withheld ? "withheld" : f.level}</span>
              </button>
            </li>
          )}
        </For>
      </ul>
      <div class="viewer" aria-live="polite">
        <Show when={selected()} fallback={<p class="muted">Choose a file. Files marked <em>withheld</em> were never shown to the agent and are not shown here.</p>}>
          <Show when={content()}>
            {(c) => (
              <Show when={!c().withheld} fallback={<div class="redacted"><strong>Withheld.</strong> This file is labelled restricted. The agent could not read it and it will not be released.</div>}>
                <pre tabindex="0"><code>{c().text}</code></pre>
              </Show>
            )}
          </Show>
        </Show>
      </div>
    </div>
  );
}

function Findings(props: { job: Job }) {
  const [data] = createResource(() => props.job.id, (id) => api.findings(id));
  return (
    <div class="stack">
      <Show when={data()} fallback={<p class="muted">Loading…</p>}>
        {(d) => (
          <>
            <Show when={d().findings.length === 0}>
              <div class="card"><p>The agent reported no findings.</p><p class="muted">That is not proof the project is free of defects; it only means nothing was reported and confirmed.</p></div>
            </Show>
            <For each={d().findings}>
              {(f) => {
                const [t, cls] = statusText(f.status);
                return (
                  <article class="card stack">
                    <header class="between"><h2>{f.title}</h2><span class={`chip ${cls}`}>{t}</span></header>
                    <p><code>{f.file}:{f.line}</code></p>
                    <p>{f.explanation}</p>
                    <Show when={f.reproducer}>
                      <details><summary>Reproducer the kernel ran</summary><pre tabindex="0"><code>{f.reproducer}</code></pre><Show when={f.output}><p class="muted">Output</p><pre tabindex="0"><code>{f.output}</code></pre></Show></details>
                    </Show>
                    <Show when={f.sample.length > 0}><p class="muted">Checked on a minimal sample of {f.sample.length} files: {f.sample.join(", ")}</p></Show>
                  </article>
                );
              }}
            </For>
            <Show when={d().unparsed > 0}><p class="muted">{d().unparsed} report(s) from the agent were malformed and ignored.</p></Show>
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

  const poll = async () => {
    try {
      setApproval(await api.approval(props.job.id));
    } catch {
      /* try again on the next tick */
    }
  };
  createEffect(() => {
    void poll();
    const t = setInterval(poll, 1000);
    onCleanup(() => clearInterval(t));
  });

  const ask = async () => {
    setBusy(true);
    setError("");
    try {
      const r = await api.requestApproval(props.job.id);
      setHow(r.how);
      await poll();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Could not request approval.");
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
      setError(e instanceof ApiError ? e.message : "Delivery failed.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="stack">
      <Show when={release()} fallback={<p class="muted">Loading…</p>}>
        {(r) => (
          <Show when={r().available} fallback={
            <div class="card stack">
              <h2>Nothing to release</h2>
              <p>No finding was confirmed by the kernel, so nothing can be sent. This is the safe result.</p>
              <For each={r().not_confirmed}>{(n) => <p class="muted">{n.title}: {n.why}</p>}</For>
            </div>
          }>
            <div class="card stack">
              <h2>The exact release</h2>
              <p>For <strong>{r().recipient}</strong>. Digest <code>{r().digest}</code></p>
              <table>
                <thead><tr><th>File</th><th>Size</th><th>SHA-256</th></tr></thead>
                <tbody><For each={r().files}>{(f) => <tr><td><code>{f.name}</code></td><td>{f.bytes} bytes</td><td><code>{f.sha256.slice(0, 16)}</code></td></tr>}</For></tbody>
              </table>
              <For each={r().files!.filter((f) => f.name.startsWith("finding-"))}>{(f) => <details open><summary>{f.name}</summary><pre tabindex="0"><code>{f.text}</code></pre></details>}</For>
              <h3>Held back</h3>
              <ul class="plain"><For each={r().removed} fallback={<li class="muted">Nothing.</li>}>{(x) => <li>{x.what}: <span class="muted">{x.why}</span></li>}</For></ul>
              <Show when={r().not_confirmed.length > 0}>
                <h3>Not confirmed, so not included</h3>
                <ul class="plain"><For each={r().not_confirmed}>{(x) => <li>{x.title} <span class="muted">({x.why})</span></li>}</For></ul>
              </Show>
              <h3>This release does not establish</h3>
              <ul class="plain"><For each={r().not_established}>{(x) => <li>{x}</li>}</For></ul>
            </div>
            <div class="card stack" aria-live="polite">
              <h2>Approval</h2>
              <Show when={approval().state === "none"}>
                <p>The browser cannot approve a release by itself. You approve in a terminal on this computer, where you see the exact bytes again.</p>
                <button class="primary" disabled={busy()} onClick={ask}>Request approval</button>
              </Show>
              <Show when={approval().state === "waiting"}>
                <p><strong>Waiting for you.</strong> Run this in a terminal on this computer:</p>
                <pre class="cmd" tabindex="0"><code>kaku approve {approval().approval}</code></pre>
                <p class="muted">Type the first 12 characters of the digest shown there. {how()}</p>
              </Show>
              <Show when={approval().state === "approved"}>
                <p class="ok">Approved in the terminal. The files are fixed; delivery re-checks that nothing changed.</p>
                <button class="primary" disabled={busy()} onClick={deliver}>Sign and deliver the capsule</button>
              </Show>
              <Show when={approval().state === "denied" || approval().state === "revoked"}><p class="bad">The release was refused. Nothing was sent.</p></Show>
              <Show when={approval().state === "uncertain"}><p class="warn">Delivery was interrupted and its outcome is unknown. It will not be retried automatically.</p></Show>
              <Show when={props.job.delivered}>
                {(d) => (
                  <div class="stack">
                    <p class="ok"><strong>Delivered.</strong> Capsule <code>{d().name}</code> signed by <code>{d().signer.slice(0, 24)}…</code></p>
                    <p><a class="button" href={`/api/jobs/${props.job.id}/capsule`} download="review.kkg">Download the capsule</a></p>
                    <p class="muted">Give the recipient the file. They can check it offline at <a href="/verify" target="_blank" rel="noopener">/verify</a> or with <code>kaku verify</code>.</p>
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
    const t = setInterval(load, 700);
    onCleanup(() => clearInterval(t));
  });
  const tabs: [Tab, string][] = [["progress", "Progress"], ["protected", "Protection"], ["source", "Source"], ["findings", "Findings"], ["release", "Release"]];
  return (
    <Show when={job()} fallback={<p class="muted">Loading…</p>}>
      {(j) => (
        <section class="stack wide" aria-labelledby="h-job">
          <header class="between">
            <div>
              <h1 id="h-job">{j().repo.split("/").pop()}</h1>
              <p class="muted">for {j().recipient} · <span class={`state-text ${j().state}`}>{j().state.replace(/_/g, " ")}</span></p>
            </div>
          </header>
          <div class="tabs" role="tablist">
            <For each={tabs}>{([k, label]) => <button role="tab" aria-selected={tab() === k} class={tab() === k ? "tab active" : "tab"} onClick={() => setTab(k)}>{label}</button>}</For>
          </div>
          <div role="tabpanel">
            <Show when={tab() === "progress"}><Progress job={j()} /></Show>
            <Show when={tab() === "protected"}><Protected job={j()} /></Show>
            <Show when={tab() === "source"}><Show when={j().summary} fallback={<p class="muted">Available when the review finishes.</p>}><Source job={j()} /></Show></Show>
            <Show when={tab() === "findings"}><Show when={j().summary} fallback={<p class="muted">Available when the review finishes.</p>}><Findings job={j()} /></Show></Show>
            <Show when={tab() === "release"}><Show when={j().summary} fallback={<p class="muted">Available when the review finishes.</p>}><ReleaseTab job={j()} onChange={load} /></Show></Show>
          </div>
        </section>
      )}
    </Show>
  );
}
