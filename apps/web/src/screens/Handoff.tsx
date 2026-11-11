import { createResource, createSignal, Show } from "solid-js";
import { api, ApiError, type HandoffProposal } from "../api";
import { t } from "../i18n";

/** Handing text to the coding agent installed on this computer: write, read back exactly what will go, then send. */
export function Handoff() {
  const [status] = createResource(() => api.handoffStatus().catch(() => null));
  const [text, setText] = createSignal("");
  const [shown, setShown] = createSignal<HandoffProposal | null>(null);
  const [reply, setReply] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");

  const show = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    setReply("");
    try {
      setShown(await api.handoffPropose(text()));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("hand.error"));
    } finally {
      setBusy(false);
    }
  };
  const send = async () => {
    const p = shown();
    if (!p) return;
    setBusy(true);
    setError("");
    try {
      const r = await api.handoffRun(p.id);
      setReply(r.reply);
      setShown(null);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("hand.error"));
      setShown(null);
    } finally {
      setBusy(false);
    }
  };
  const cancel = async () => {
    const p = shown();
    setShown(null);
    if (p) await api.handoffCancel(p.id).catch(() => undefined);
  };

  return (
    <section class="stack wide" aria-labelledby="h-hand">
      <h1 id="h-hand">{t("hand.title")}</h1>
      <p class="lead">{t("hand.lead")}</p>
      <Show when={status()} fallback={<p class="muted" role="status">{status.loading ? t("review.loading") : t("hand.error")}</p>}>
        {(s) => (
          <Show when={s().available} fallback={<p class="card" id="hand-none">{t("hand.none")}</p>}>
            <p class="banner-note" id="hand-notice">{t("hand.notice")}</p>
            <Show when={!shown()}>
              <form class="card stack" onSubmit={(e) => void show(e)}>
                <label for="hand-text">{t("hand.text", { tool: s().tool ?? "" })}</label>
                <textarea id="hand-text" rows="8" maxlength="20000" value={text()} placeholder={t("hand.hint")} onInput={(e) => setText(e.currentTarget.value)} />
                <div class="actions">
                  <button class="primary" type="submit" disabled={busy() || text().trim().length === 0}>{t("hand.show")}</button>
                </div>
              </form>
            </Show>
            <Show when={shown()}>
              {(p) => (
                <div class="card stack" id="hand-confirm" role="group" aria-labelledby="h-hand-confirm">
                  <h2 id="h-hand-confirm">{t("hand.confirm", { tool: p().tool })}</h2>
                  <pre class="wrap" id="hand-exact">{p().text}</pre>
                  <ul class="plain muted">
                    <li>{t("hand.size", { n: p().chars })}</li>
                    <Show when={p().personal}><li class="warn">{t("hand.personal", { what: p().personal })}</li></Show>
                    <li>{t("hand.digest")} <code class="wrap">{p().sha256}</code></li>
                  </ul>
                  <div class="actions">
                    <button class="primary" type="button" disabled={busy()} onClick={() => void send()}>{busy() ? t("hand.sending") : t("hand.send", { tool: p().tool })}</button>
                    <button type="button" disabled={busy()} onClick={() => void cancel()}>{t("hand.cancel")}</button>
                  </div>
                </div>
              )}
            </Show>
          </Show>
        )}
      </Show>
      <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
      <Show when={reply()}>
        <div class="card stack" id="hand-reply" aria-labelledby="h-hand-reply">
          <h2 id="h-hand-reply">{t("hand.reply")}</h2>
          <p class="muted">{t("hand.replyNote")}</p>
          <pre class="wrap">{reply()}</pre>
        </div>
      </Show>
    </section>
  );
}
