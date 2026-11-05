import { batch, createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, streamChat, type ChatTurn, type KeptChat, type GatewayProofResult, type ModelProof, type ResearchPage, type ResearchProposal, type SearchHit, type SearchProposal } from "../api";
import { t } from "../i18n";
import { Markdown } from "../components/Markdown";
import { draftKey, forgetDraft, forgetSent, historyKey, historyStep, idleHistory, loadDraft, rememberSent, retagSent, saveDraft, sentHistory, stopsReply } from "../composer";
import { meterLine, pageNotes } from "../meter";

interface Shown extends ChatTurn {
  foot?: string[];
  notes?: { id: string; text: string; why: string; untrusted: boolean; project?: string | null }[];
}

export function Chat() {
  const [turns, setTurns] = createSignal<Shown[]>([]);
  const [text, setTextRaw] = createSignal(loadDraft(draftKey(null)));
  const [busy, setBusy] = createSignal(false);
  const [live, setLive] = createSignal("");
  let abort: AbortController | null = null;
  const [error, setError] = createSignal("");
  const [session, setSession] = createSignal<string | null>(null);
  const [confirmForget, setConfirmForget] = createSignal<string | null>(null);
  // The box holds the draft of the conversation on screen. Each conversation keeps its own while the page is open.
  const setText = (v: string) => {
    setTextRaw(v);
    saveDraft(draftKey(session()), v);
  };
  // Bumped whenever another conversation is put on screen, so a reply that arrives late never lands in the wrong one.
  let view = 0;
  // Where Up and Down are in the messages sent from this page.
  let hist = idleHistory();
  let box: HTMLTextAreaElement | undefined;
  const show = (next: string | null, nextTurns: Shown[]) => {
    if (busy()) abort?.abort();
    view++;
    hist = idleHistory();
    batch(() => {
      setSession(next);
      setTurns(nextTurns);
      setTextRaw(loadDraft(draftKey(next)));
    });
  };
  // After a reply, the cursor goes back to the box, unless the owner has moved on to something else or is on a phone.
  const returnFocus = () => {
    if (window.matchMedia("(max-width: 767px)").matches) return;
    const a = document.activeElement;
    if (a && a !== document.body && !box?.form?.contains(a)) return;
    box?.focus();
  };
  const [kept, { refetch, mutate }] = createResource<KeptChat[]>(async () => {
    try {
      return (await api.keptChats()).sessions;
    } catch {
      return [];
    }
  });
  const keep = async () => {
    setError("");
    try {
      const r = await api.keepChat("");
      const carried = text();
      forgetDraft(draftKey(session()));
      retagSent(draftKey(session()), draftKey(r.id));
      setSession(r.id);
      saveDraft(draftKey(r.id), carried);
      await refetch();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("chat.error"));
    }
  };
  const openKept = async (id: string) => {
    setError("");
    try {
      const r = await api.keptChat(id);
      show(id, r.turns.map(({ role, content }) => ({ role, content })));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("chat.error"));
    }
  };
  const forget = async (id: string) => {
    if (confirmForget() !== id) {
      setConfirmForget(id);
      return;
    }
    setConfirmForget(null);
    try {
      await api.forgetChat(id);
      // the list and the screen change together: the forgotten conversation is never listed once its text is gone
      batch(() => {
        mutate((list) => (list ?? []).filter((k) => k.id !== id));
        forgetDraft(draftKey(id));
        forgetSent(draftKey(id));
        hist = idleHistory();
        if (session() === id) show(null, []);
      });
      await refetch();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("chat.error"));
    }
  };
  const [proofModel, setProofModel] = createSignal("Qwen/Qwen3.8-27B");
  const [proof, setProof] = createSignal<ModelProof | null>(null);
  const [gateway, setGateway] = createSignal<GatewayProofResult | null>(null);
  const [proofBusy, setProofBusy] = createSignal(false);
  const [proofError, setProofError] = createSignal("");
  const checkProof = async () => {
    setProofBusy(true);
    setProofError("");
    setProof(null);
    try {
      setProof(await api.attestModel(proofModel().trim()));
    } catch (err) {
      setProofError(err instanceof ApiError ? err.message : t("proof.error"));
    } finally {
      setProofBusy(false);
    }
  };
  const checkGateway = async () => {
    setProofBusy(true);
    setProofError("");
    setGateway(null);
    try {
      setGateway(await api.attestGateway());
    } catch (err) {
      setProofError(err instanceof ApiError ? err.message : t("proof.error"));
    } finally {
      setProofBusy(false);
    }
  };
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
  const [searchOn, setSearchOn] = createSignal<{ available: boolean; provider?: string } | null>(null);
  api.searchStatus().then(setSearchOn).catch(() => setSearchOn({ available: false }));
  const [words, setWords] = createSignal("");
  const [searchProposal, setSearchProposal] = createSignal<SearchProposal | null>(null);
  const [hits, setHits] = createSignal<SearchHit[]>([]);
  const showSearch = async () => {
    setWebBusy(true);
    setWebError("");
    setSearchProposal(null);
    setHits([]);
    try {
      setSearchProposal(await api.searchPropose(words()));
    } catch (err) {
      setWebError(err instanceof ApiError ? err.message : t("web.error"));
    } finally {
      setWebBusy(false);
    }
  };
  const sendSearch = async () => {
    const p = searchProposal();
    if (!p) return;
    setWebBusy(true);
    setWebError("");
    try {
      setHits((await api.searchRun(p.id, p.digest)).results);
      setSearchProposal(null);
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
    const myView = view;
    const sentKey = draftKey(session());
    rememberSent(message, sentKey);
    hist = idleHistory();
    const history: ChatTurn[] = turns().slice(-18).map(({ role, content }) => ({ role, content }));
    setTurns([...turns(), { role: "user", content: message }]);
    setText("");
    setBusy(true);
    setError("");
    setLive("");
    abort = new AbortController();
    try {
      const r = await streamChat({ message, history, sources: pages().filter((p) => p.page_id && !p.looks_like_instructions).map((p) => p.page_id as string), ...(session() ? { session: session() } : {}) }, (d) => setLive((x) => x + d), abort.signal);
      if (myView !== view) {
        if ("stopped" in r) saveDraft(sentKey, message);
        return;
      }
      if ("stopped" in r) {
        setTurns(turns().slice(0, -1));
        setText(message);
        return;
      }
      const foot = [r.route.simulated ? t("chat.test") : t("chat.private"), t("chat.notesUsed", { n: r.notes_used })];
      if (r.usage) {
        const m = meterLine(r.usage);
        foot.push(t(m.key, { n: m.n, max: m.max ?? 0, pct: m.pct ?? 0 }));
        if (m.near) foot.push(t("chat.contextNear"));
        if (r.usage.history_turns_dropped > 0) foot.push(t("chat.dropped", { n: r.usage.history_turns_dropped }));
      }
      if (session()) foot.push(r.saved ? t("chat.saved") : t("chat.notSaved"));
      for (const p of pageNotes(r)) foot.push(t(p.key, { n: p.n }));
      if (r.notes_withheld > 0) foot.push(t("chat.withheld", { n: r.notes_withheld }));
      setTurns([...turns(), { role: "assistant", content: r.reply, foot, notes: r.notes }]);
      if (session()) await refetch();
    } catch (err) {
      // a message whose reply was cut short by switching conversations goes back to the draft it was written in
      if (myView !== view) {
        saveDraft(sentKey, message);
        return;
      }
      setTurns(turns().slice(0, -1));
      setText(message);
      if (!(err instanceof DOMException && err.name === "AbortError")) setError(err instanceof ApiError ? err.message : t("chat.error"));
    } finally {
      setBusy(false);
      setLive("");
      abort = null;
      if (myView === view) returnFocus();
    }
  };
  return (
    <section class="stack wide" aria-labelledby="h-chat">
      <h1 id="h-chat">{t("chat.title")}</h1>
      <p class="lead">{t("chat.lead")}</p>
      <p class="muted">{session() ? t("chat.noteKept") : t("chat.note")}</p>
      <div class="card stack" role="region" aria-label={t("chat.keptTitle")}>
        <div class="actions">
          <Show when={!session()}><button type="button" onClick={keep}>{t("chat.keep")}</button></Show>
          <Show when={session()}><span class="chip ok">{t("chat.keptNow")}</span> <button type="button" onClick={() => show(null, [])}>{t("chat.newChat")}</button></Show>
        </div>
        <Show when={(kept() ?? []).length > 0}>
          <p><strong>{t("chat.keptTitle")}</strong></p>
          <ul class="plain stack">
            <For each={kept() ?? []}>
              {(k) => (
                <li class="actions">
                  <span>{k.title || t("chat.untitled")} · {t("chat.turns", { n: k.turns })}</span>
                  <button type="button" onClick={() => openKept(k.id)}>{t("chat.open")}</button>
                  <button type="button" onClick={() => forget(k.id)}>{confirmForget() === k.id ? t("chat.forgetSure") : t("chat.forget")}</button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
      <ul class="plain stack chat" aria-live="polite">
        <For each={turns()} fallback={<li class="muted">{t("chat.empty")}</li>}>
          {(m) => (
            <li class={`card stack msg ${m.role}`}>
              <strong>{m.role === "user" ? t("chat.you") : t("chat.assistant")}</strong>
              {m.role === "assistant" ? <Markdown text={m.content} /> : <p class="mem-text">{m.content}</p>}
              <Show when={m.foot}>{(f) => <p class="muted">{f().join(" · ")}</p>}</Show>
              <Show when={(m.notes ?? []).length > 0}>
                <details class="notes-used">
                  <summary>{t("chat.whichNotes", { n: m.notes!.length })}</summary>
                  <ul class="plain"><For each={m.notes}>{(n) => <li><span>{n.text}</span> <span class="muted">({n.why}{n.untrusted ? `; ${t("mem.untrusted")}` : ""})</span></li>}</For></ul>
                </details>
              </Show>
            </li>
          )}
        </For>
        <Show when={busy() && live()}><li class="card stack msg assistant"><strong>{t("chat.assistant")}</strong><Markdown text={live()} /></li></Show>
        <Show when={busy() && !live()}><li class="muted" role="status">{t("chat.thinking")}</li></Show>
      </ul>
      <details class="card stack" id="model-proof">
        <summary>{t("proof.title")}</summary>
        <p class="muted">{t("proof.lead")}</p>
        <label for="proof-model">{t("proof.model")}</label>
        <input id="proof-model" type="text" value={proofModel()} onInput={(e) => setProofModel(e.currentTarget.value)} />
        <div class="actions">
          <button type="button" disabled={proofBusy() || proofModel().trim().length === 0} onClick={checkProof}>{proofBusy() ? t("proof.checking") : t("proof.check")}</button>
          <button type="button" disabled={proofBusy()} onClick={checkGateway}>{t("proof.checkGateway")}</button>
          <Show when={proofError()}><span role="alert" class="bad">{proofError()}</span></Show>
        </div>
        <Show when={gateway()}>
          {(g) => (
            <div class="stack" role="region" aria-label={t("proof.gatewayResult")}>
              <p><span class={`chip ${g().verified ? "ok" : "bad"}`}>{g().verified ? t("proof.verified") : t("proof.notVerified")}</span> {t("proof.gateway")}</p>
              <Show when={g().gateway}>
                {(d) => (
                  <ul class="plain">
                    <li>{t("proof.tcb", { v: d().tcb })}</li>
                    <li>{d().tls_bound ? t("proof.tlsYes") : t("proof.tlsNo")}</li>
                    <li>{t("proof.signer")} <code>{d().signer}</code></li>
                    <li>{t("proof.nonce")} <code>{d().nonce}</code></li>
                  </ul>
                )}
              </Show>
              <p><strong>{t("proof.means")}</strong></p>
              <ul><For each={g().means ?? []}>{(m) => <li>{m}</li>}</For></ul>
              <p><strong>{t("proof.notMeans")}</strong></p>
              <ul><For each={g().does_not_mean ?? []}>{(m) => <li>{m}</li>}</For></ul>
            </div>
          )}
        </Show>
        <Show when={proof()}>
          {(p) => (
            <div class="stack" role="region" aria-label={t("proof.result")}>
              <p><span class={`chip ${p().verified ? "ok" : "bad"}`}>{p().verified ? t("proof.verified") : t("proof.notVerified")}</span> <code>{p().model}</code></p>
              <For each={p().deployments ?? []}>
                {(d) => (
                  <ul class="plain">
                    <li>{t("proof.tcb", { v: d.tcb })}</li>
                    <li>{d.gpu_verified ? t("proof.gpuYes") : t("proof.gpuNo")}</li>
                    <li>{t("proof.signer")} <code>{d.signer}</code></li>
                    <Show when={d.compose_hash}><li>{t("proof.compose")} <code>{d.compose_hash}</code></li></Show>
                    <li>{t("proof.nonce")} <code>{d.nonce}</code></li>
                  </ul>
                )}
              </For>
              <p><strong>{t("proof.means")}</strong></p>
              <ul><For each={p().means ?? []}>{(m) => <li>{m}</li>}</For></ul>
              <p><strong>{t("proof.notMeans")}</strong></p>
              <ul><For each={p().does_not_mean ?? []}>{(m) => <li>{m}</li>}</For></ul>
            </div>
          )}
        </Show>
      </details>
      <details class="card stack" id="web-research">
        <summary>{t("web.title")}</summary>
        <p class="muted">{t("web.lead")}</p>
        <Show when={searchOn()?.available}>
          <label for="web-search">{t("web.searchWords")}</label>
          <input id="web-search" type="text" maxlength="200" value={words()} onInput={(e) => setWords(e.currentTarget.value)} />
          <div class="actions">
            <button type="button" disabled={webBusy() || words().trim().length === 0} onClick={showSearch}>{t("web.searchShow")}</button>
          </div>
          <Show when={searchProposal()}>
            {(p) => (
              <div class="stack" role="region" aria-label={t("web.searchReview")}>
                <p><strong>{t("web.searchSends", { site: p().will_send_to })}</strong></p>
                <p><code>{p().query}</code></p>
                <p class="muted">{p().stays_here}</p>
                <div class="actions">
                  <button class="primary" type="button" disabled={webBusy()} onClick={sendSearch}>{t("web.searchSend")}</button>
                  <button type="button" onClick={() => setSearchProposal(null)}>{t("web.cancel")}</button>
                </div>
              </div>
            )}
          </Show>
          <Show when={hits().length > 0}>
            <p><strong>{t("web.searchResults")}</strong></p>
            <ul class="plain">
              <For each={hits()}>
                {(h) => (
                  <li>
                    <code>{h.url}</code>{h.withheld ? ` — ${t("web.withheld")}` : ` · ${h.title}`}
                    <Show when={h.snippet}><span class="muted"> — {h.snippet}</span></Show>
                    <button type="button" onClick={() => setUrls((urls() ? urls() + "\n" : "") + h.url)}>{t("web.searchUse")}</button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
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
          ref={box}
          id="chat-input"
          rows="3"
          value={text()}
          onInput={(e) => setText(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (stopsReply(e, busy())) {
              e.preventDefault();
              abort?.abort();
              return;
            }
            const dir = historyKey(e);
            if (dir) {
              const el = e.currentTarget;
              if (el.selectionStart !== el.selectionEnd) return;
              const step = historyStep(sentHistory(), hist, dir, el.value, el.selectionStart);
              if (step) {
                e.preventDefault();
                hist = step.state;
                setText(step.text);
                el.value = step.text;
                el.setSelectionRange(step.cursor, step.cursor);
              }
              return;
            }
            // Enter that confirms an input-method composition (Hindi, Chinese, Japanese) must not send the message.
            if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.keyCode !== 229) {
              e.preventDefault();
              void send();
            }
          }}
        />
        <div class="actions">
          <button class="primary" type="submit" disabled={busy() || text().trim().length === 0}>{t("chat.send")}</button>
          <Show when={busy()}><button type="button" onClick={() => abort?.abort()}>{t("chat.stop")}</button></Show>
          <button type="button" disabled={busy() || turns().length === 0} onClick={() => { setTurns([]); setError(""); forgetSent(draftKey(session())); hist = idleHistory(); }}>{t("chat.clear")}</button>
          <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
        </div>
      </form>
    </section>
  );
}
