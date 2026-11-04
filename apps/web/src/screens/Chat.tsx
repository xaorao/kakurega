import { createSignal, For, Show } from "solid-js";
import { api, ApiError, type ChatTurn } from "../api";
import { t } from "../i18n";

interface Shown extends ChatTurn {
  foot?: string[];
}

export function Chat() {
  const [turns, setTurns] = createSignal<Shown[]>([]);
  const [text, setText] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const send = async (e?: Event) => {
    e?.preventDefault();
    const message = text().trim();
    if (!message || busy()) return;
    const history: ChatTurn[] = turns().slice(-18).map(({ role, content }) => ({ role, content }));
    setTurns([...turns(), { role: "user", content: message }]);
    setText("");
    setBusy(true);
    setError("");
    try {
      const r = await api.chat(message, history);
      const foot = [r.route.simulated ? t("chat.test") : t("chat.private"), t("chat.notesUsed", { n: r.notes_used })];
      if (r.notes_withheld > 0) foot.push(t("chat.withheld", { n: r.notes_withheld }));
      setTurns([...turns(), { role: "assistant", content: r.reply, foot }]);
    } catch (err) {
      setTurns(turns().slice(0, -1));
      setText(message);
      setError(err instanceof ApiError ? err.message : t("chat.error"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section class="stack wide" aria-labelledby="h-chat">
      <h1 id="h-chat">{t("chat.title")}</h1>
      <p class="lead">{t("chat.lead")}</p>
      <p class="muted">{t("chat.note")}</p>
      <ul class="plain stack chat" aria-live="polite">
        <For each={turns()} fallback={<li class="muted">{t("chat.empty")}</li>}>
          {(m) => (
            <li class={`card stack msg ${m.role}`}>
              <strong>{m.role === "user" ? t("chat.you") : t("chat.assistant")}</strong>
              <p class="mem-text">{m.content}</p>
              <Show when={m.foot}>{(f) => <p class="muted">{f().join(" · ")}</p>}</Show>
            </li>
          )}
        </For>
        <Show when={busy()}><li class="muted" role="status">{t("chat.thinking")}</li></Show>
      </ul>
      <form class="card stack" onSubmit={send}>
        <label for="chat-input">{t("chat.input")}</label>
        <textarea
          id="chat-input"
          rows="3"
          value={text()}
          onInput={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => {
            // Enter that confirms an input-method composition (Hindi, Chinese, Japanese) must not send the message.
            if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.keyCode !== 229) {
              e.preventDefault();
              void send();
            }
          }}
        />
        <div class="actions">
          <button class="primary" type="submit" disabled={busy() || text().trim().length === 0}>{t("chat.send")}</button>
          <button type="button" disabled={busy() || turns().length === 0} onClick={() => { setTurns([]); setError(""); }}>{t("chat.clear")}</button>
          <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
        </div>
      </form>
    </section>
  );
}
