import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type Config, type Readiness } from "../api";

export function NewReview(props: { config: Config; onStarted: (id: string) => void }) {
  const [dir, setDir] = createSignal<string | undefined>(undefined);
  const [listing] = createResource(() => dir() ?? "", (d) => api.fs(d || undefined));
  const [recipient, setRecipient] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const chosen = () => listing()?.path ?? "";
  const [ready, { refetch }] = createResource<Readiness | null>(() => api.readiness().catch(() => null));
  const blocked = () => ready()?.ready === false;

  const start = async () => {
    setBusy(true);
    setError("");
    try {
      const r = await api.startJob(chosen(), recipient().trim());
      props.onStarted(r.id);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : "Could not start the review.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <section class="stack wide" aria-labelledby="h-new">
      <h1 id="h-new">Review a project</h1>
      <p class="lead">
        An agent reads the project inside a sealed sandbox and looks for real defects. Secrets are withheld from it. Only findings that the kernel can reproduce on a small extract are put in the release, and nothing leaves until you approve the exact files.
      </p>
      <Show when={ready()}>
        {(r) => (
          <div class="card stack" role="region" aria-label="Setup check">
            <h2>{r().ready ? "Ready to review" : "Finish setup first"}</h2>
            <ul class="checks">
              <For each={r().checks}>
                {(c) => (
                  <li>
                    <span class={c.ok ? "ok-text" : "bad"}>{c.ok ? "Ready" : "Needs attention"}</span> <strong>{c.label}</strong>: {c.detail}
                    <Show when={c.fix}><div class="fix"><code>{c.fix}</code></div></Show>
                  </li>
                )}
              </For>
            </ul>
            <Show when={!r().ready}>
              <div class="actions"><button onClick={() => refetch()}>Check again</button></div>
            </Show>
          </div>
        )}
      </Show>
      <div class="card stack">
        <h2>1. Choose the project folder</h2>
        <p class="path" aria-live="polite">{chosen() || "Loading…"}</p>
        <div class="browser" role="group" aria-label="Folders">
          <Show when={listing()?.parent}>
            {(p) => <button class="row" onClick={() => setDir(p())}>↑ Up</button>}
          </Show>
          <For each={listing()?.dirs} fallback={<p class="muted">{listing.loading ? "Loading folders…" : "No sub-folders here."}</p>}>
            {(d) => <button class="row" onClick={() => setDir(d.path)}>{d.name}</button>}
          </For>
        </div>
      </div>
      <div class="card stack">
        <h2>2. Who will receive the result?</h2>
        <label for="recipient">Recipient name</label>
        <input id="recipient" placeholder="for example: acme-audit" value={recipient()} onInput={(e) => setRecipient(e.currentTarget.value)} />
        <p class="muted">The release and the signed capsule are bound to this name. A capsule made for one recipient is rejected for another.</p>
      </div>
      <div class="actions">
        <button class="primary" disabled={busy() || blocked() || !chosen() || recipient().trim().length === 0} onClick={start}>Start review</button>
        <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
      </div>
      <p class="muted">Model: <strong>{props.config.route.model ?? props.config.route.id}</strong>{props.config.route.simulated ? " (scripted, test mode)" : " (runs on this computer)"}. Small local models find fewer defects than hosted ones; the kernel will not release a finding it cannot reproduce.</p>
    </section>
  );
}
