import { createSignal, For, Show } from "solid-js";
import { api, ApiError, type ChatTurn, type ResearchPage, type ResearchProposal } from "../api";
import { t } from "../i18n";

interface Shown extends ChatTurn {
  foot?: string[];
}

export function Chat() {
  const [turns, setTurns] = createSignal<Shown[]>([]);
  const [text, setText] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [urls, setUrls] = createSignal("");
  const [proposal, setProposal] = createSignal<ResearchProposal | null>(null);
  const [pages, setPages] = createSignal<ResearchPage[]>([]);
  const [webBusy, setWebBusy] = createSignal(false);
  const [webError, setWebError] = createSignal("");
  const propose = async () => {
    setWebBusy(true);
    setWebError("");
    setProposal(null);
    try {
      const list = urls().split(/\s+/).filter(Boolean);
      setProposal(await api.researchPropose(list));
    } catch (err) {
      setWebError(err instanceof ApiError ? err.message : t("web.error"));
    } finally {
      setWebBusy(false);
    }
  };
  const fetchApproved = async () => {
    const p = proposal();
    if (!p) return;
    setWebBusy(true);
    setWebError("");
    try {
      const r = await api.researchFetch(p.id, p.digest);
      setPages([...pages(), ...r.pages]);
      setProposal(null);
      setUrls("");
    } catch (err) {
      setWebError(err instanceof ApiError ? err.message : t("web.error"));
    } finally {
      setWebBusy(false);
    }
  };
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
      const r = await api.chat(message, history, pages().filter((p) => p.page_id && !p.looks_like_instructions).map((p) => p.page_id as string));
      const foot = [r.route.simulated ? t("chat.test") : t("chat.private"), t("chat.notesUsed", { n: r.notes_used })];
      if (r.web_sources_used > 0) foot.push(t("web.used", { n: r.web_sources_used }));
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
      <details class="card stack" id="web-research">
        <summary>{t("web.title")}</summary>
        <p class="muted">{t("web.lead")}</p>
        <label for="web-urls">{t("web.urls")}</label>
        <textarea id="web-urls" rows="3" value={urls()} onInput={(e) => setUrls(e.currentTarget.value)} />
        <div class="actions">
          <button type="button" disabled={webBusy() || urls().trim().length === 0} onClick={propose}>{t("web.show")}</button>
          <Show when={webError()}><span role="alert" class="bad">{webError()}</span></Show>
        </div>
        <Show when={proposal()}>
          {(p) => (
            <div class="stack" role="region" aria-label={t("web.review")}>
              <p><strong>{t("web.willRequest")}</strong></p>
              <ul class="plain">
                <For each={p().items}>{(i) => <li class={i.allowed ? "" : "muted"}><code>{i.url}</code> {i.allowed ? "" : `— ${t("web.refused")}: ${i.reason}`}</li>}</For>
              </ul>
              <p class="muted">{p().stays_here}</p>
              <div class="actions">
                <button class="primary" type="button" disabled={webBusy()} onClick={fetchApproved}>{t("web.fetch")}</button>
                <button type="button" onClick={() => setProposal(null)}>{t("web.cancel")}</button>
              </div>
            </div>
          )}
        </Show>
        <Show when={pages().length > 0}>
          <p><strong>{t("web.pages")}</strong></p>
          <ul class="plain">
            <For each={pages()}>
              {(p) => (
                <li>
                  <code>{p.final_url ?? p.url}</code> {p.title ? `· ${p.title}` : ""}
                  {p.error ? ` — ${t("web.failed")}: ${p.error}` : ""}
                  {p.looks_like_instructions ? ` — ${t("web.withheld")}` : ""}
                </li>
              )}
            </For>
          </ul>
          <button type="button" onClick={() => setPages([])}>{t("web.forget")}</button>
        </Show>
      </details>
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
