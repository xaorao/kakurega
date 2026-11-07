import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type ChatSent } from "../api";
import { t } from "../i18n";
import { modelKey } from "../privacy";
import { sameWords } from "../sent";

/** What was handed to the model for the latest answer, word for word, next to what was typed. Fetched only when opened; only the latest call is kept. */
export function SentView(props: { typed: string }) {
  const [open, setOpen] = createSignal(false);
  const [sent] = createResource(open, async (o) => {
    if (!o) return null;
    try {
      return await api.chatSent();
    } catch (e) {
      return e instanceof ApiError && e.status === 404 ? "gone" : "error";
    }
  });
  const data = (): ChatSent | null => {
    const v = sent();
    return v && typeof v === "object" ? v : null;
  };
  return (
    <details class="sent-view" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>{t("sent.summary")}</summary>
      <Show when={open()}>
        <Show when={data()} fallback={<p class="muted" role="status">{sent.loading ? t("sent.loading") : sent() === "gone" ? t("sent.gone") : t("sent.error")}</p>}>
          {(d) => (
            <div class="stack">
              <p>{t(modelKey({ class: d().route.class, simulated: d().route.simulated, host: d().route.host, stays_here: d().route.stays_here }), { name: d().route.name ?? d().route.id, host: d().route.host ?? "" })}</p>
              <p class="muted">{sameWords(d().messages, props.typed) ? t("sent.same") : t("sent.differs")}</p>
              <For each={d().messages}>
                {(m) => (
                  <div>
                    <strong>{t(`sent.role.${m.role === "system" || m.role === "user" || m.role === "assistant" ? m.role : "other"}` as never)}</strong>
                    <pre class="sent-text">{m.content}</pre>
                  </div>
                )}
              </For>
              <Show when={(d().excluded_notes ?? []).length > 0}>
                <div>
                  <strong>{t("sent.excluded")}</strong>
                  <ul class="plain">
                    <For each={d().excluded_notes}>
                      {(n) => <li><span>{n.text}</span> <span class="muted">({t(`sent.why.${n.reason === "looks_like_instruction" || n.reason === "did_not_fit" || n.reason === "kept_on_this_computer" ? n.reason : "other"}` as never)})</span></li>}
                    </For>
                  </ul>
                </div>
              </Show>
              <Show when={d().cut}><p class="muted">{t("sent.cut")}</p></Show>
            </div>
          )}
        </Show>
      </Show>
    </details>
  );
}
