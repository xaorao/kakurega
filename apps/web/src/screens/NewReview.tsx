import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api, ApiError, type Config, type Readiness } from "../api";
import { rich, t } from "../i18n";

export function NewReview(props: { config: Config; onStarted: (id: string) => void }) {
  const [dir, setDir] = createSignal<string | undefined>(undefined);
  const [listing] = createResource(() => dir() ?? "", (d) => api.fs(d || undefined));
  const [recipient, setRecipient] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const chosen = () => listing()?.path ?? "";
  const [ready, { refetch }] = createResource<Readiness | null>(() => api.readiness().catch(() => null));
  const blocked = () => ready()?.ready === false;
  const [repairError, setRepairError] = createSignal("");
  const fixNow = async (id: string) => {
    setRepairError("");
    try {
      await api.repair(id);
      await refetch();
    } catch (e) {
      setRepairError(e instanceof ApiError ? e.message : t("setup.repairFailed"));
    }
  };
  const poll = setInterval(() => {
    if (ready()?.checks.some((c) => c.repair?.state === "running")) void refetch();
  }, 1500);
  onCleanup(() => clearInterval(poll));

  const start = async () => {
    setBusy(true);
    setError("");
    try {
      const r = await api.startJob(chosen(), recipient().trim());
      props.onStarted(r.id);
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("new.startError"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section class="stack wide" aria-labelledby="h-new">
      <h1 id="h-new">{t("new.title")}</h1>
      <p class="greeting">{t("new.greeting")}</p>
      <p class="lead">{t("new.lead")}</p>
      <Show when={ready()}>
        {(r) => (
          <div class="card stack" role="region" aria-label={t("setup.region")}>
            <h2>{r().ready ? t("setup.ready") : t("setup.finish")}</h2>
            <ul class="checks">
              <For each={r().checks}>
                {(c) => (
                  <li>
                    <span class={c.ok ? "ok-text" : "bad"}>{c.ok ? t("setup.ok") : t("setup.attention")}</span> <strong>{c.label}</strong>: {c.detail}
                    <Show when={c.fix}><div class="fix"><code>{c.fix}</code></div></Show>
                    <Show when={c.repairable && c.repair?.state !== "running"}>
                      <div class="actions"><button type="button" onClick={() => fixNow(c.id)}>{t("setup.fixNow")}</button> <span class="muted">{t("setup.fixNote")}</span></div>
                    </Show>
                    <Show when={c.repair}>{(rp) => <p role="status" class={rp().state === "failed" ? "bad" : "muted"}>{rp().state === "running" ? t("setup.fixRunning", { d: rp().detail }) : rp().state === "failed" ? t("setup.fixFailed", { d: rp().detail }) : t("setup.fixDone")}</p>}</Show>
                  </li>
                )}
              </For>
            </ul>
            <Show when={repairError()}><p role="alert" class="bad">{repairError()}</p></Show>
            <Show when={!r().ready}>
              <div class="actions"><button onClick={() => refetch()}>{t("setup.recheck")}</button></div>
            </Show>
          </div>
        )}
      </Show>
      <div class="card stack">
        <h2>{t("new.step1")}</h2>
        <p class="path" aria-live="polite">{chosen() || t("new.loading")}</p>
        <div class="browser" role="group" aria-label={t("new.folders")}>
          <Show when={listing()?.parent}>
            {(p) => <button class="row" onClick={() => setDir(p())}>{t("new.up")}</button>}
          </Show>
          <For each={listing()?.dirs} fallback={<p class="muted">{listing.loading ? t("new.loadingFolders") : t("new.noFolders")}</p>}>
            {(d) => <button class="row" onClick={() => setDir(d.path)}>{d.name}</button>}
          </For>
        </div>
      </div>
      <div class="card stack">
        <h2>{t("new.step2")}</h2>
        <label for="recipient">{t("new.recipient.label")}</label>
        <input id="recipient" placeholder={t("new.recipient.placeholder")} value={recipient()} onInput={(e) => setRecipient(e.currentTarget.value)} />
        <p class="muted">{t("new.recipient.help")}</p>
      </div>
      <div class="actions">
        <button class="primary" disabled={busy() || blocked() || !chosen() || recipient().trim().length === 0} onClick={start}>{t("new.start")}</button>
        <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
      </div>
      <p class="muted">{rich("new.model", { model: props.config.route.model ?? props.config.route.id, where: props.config.route.simulated ? t("new.model.scripted") : t("new.model.local") })}</p>
    </section>
  );
}
