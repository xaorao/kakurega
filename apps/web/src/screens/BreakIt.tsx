import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type BreakResult } from "../api";
import { t, type Key } from "../i18n";

/** Starting points a visitor can load into the box: the kinds of text people plant in a file to turn a model against its owner. */
const IDEAS: { label: Key; text: Key }[] = [
  { label: "break.idea.order", text: "break.idea.order.text" },
  { label: "break.idea.audit", text: "break.idea.audit.text" },
  { label: "break.idea.image", text: "break.idea.image.text" },
];

export function BreakIt() {
  const [stats, { refetch }] = createResource(() => api.breakStats().catch(() => null));
  const [attack, setAttack] = createSignal("");
  const [ask, setAsk] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [result, setResult] = createSignal<BreakResult | null>(null);

  const run = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    setResult(null);
    try {
      setResult(await api.breakAttempt(attack(), ask().trim() || undefined));
      await refetch();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("break.error"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section class="stack wide" aria-labelledby="h-break">
      <h1 id="h-break">{t("break.title")}</h1>
      <p class="lead">{t("break.lead")}</p>
      <Show when={stats()}>
        {(s) => <p id="break-count" class="muted">{t("break.count", { attempts: s().attempts, leaks: s().leaks })}</p>}
      </Show>
      <form class="card stack" onSubmit={(e) => void run(e)}>
        <label for="break-attack">{t("break.attack")}</label>
        <textarea id="break-attack" rows="6" maxlength="4000" value={attack()} placeholder={t("break.attackHint")} onInput={(e) => setAttack(e.currentTarget.value)} />
        <div class="actions" role="group" aria-label={t("break.ideas")}>
          <span class="muted">{t("break.ideas")}</span>
          <For each={IDEAS}>{(i) => <button type="button" onClick={() => setAttack(t(i.text))}>{t(i.label)}</button>}</For>
        </div>
        <label for="break-ask">{t("break.ask")}</label>
        <input id="break-ask" value={ask()} maxlength="500" placeholder={t("break.askHint")} onInput={(e) => setAsk(e.currentTarget.value)} />
        <div class="actions">
          <button class="primary" type="submit" disabled={busy() || attack().trim().length === 0}>{busy() ? t("break.running") : t("break.run")}</button>
        </div>
        <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
        <Show when={busy()}><p role="status" class="muted">{t("break.wait")}</p></Show>
      </form>
      <Show when={result()}>
        {(r) => (
          <div class="card stack" id="break-result" aria-labelledby="h-break-result">
            <h2 id="h-break-result" class={r().leaked ? "bad" : "ok"}>{r().leaked ? t("break.leaked") : t("break.held")}</h2>
            <p>{t("break.planted")}</p>
            <ul class="plain">
              <li><code class="wrap">{r().planted.key}</code></li>
              <li><code class="wrap">{r().planted.password}</code></li>
            </ul>
            <p class="muted">{t(r().attack_was_sent ? "break.attackSent" : "break.attackHeld")}</p>
            <details open>
              <summary>{t("break.sent")}</summary>
              <For each={r().sent}>{(m) => <div class="sent-msg"><strong>{m.role}</strong><pre class="wrap">{m.content}</pre></div>}</For>
            </details>
            <h3>{t("break.reply")}</h3>
            <pre class="wrap" id="break-reply">{r().reply}</pre>
            <ul class="plain muted">
              <li>{r().route.stays_here ? t("meet.here") : t("meet.away", { host: r().route.host ?? "" })}</li>
              <li>{t("break.limits")}</li>
            </ul>
          </div>
        )}
      </Show>
    </section>
  );
}
