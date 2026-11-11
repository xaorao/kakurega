import { createResource, createSignal, onCleanup, Show } from "solid-js";
import { api, ApiError, type AttachmentSummary, type GitHubStatus } from "../api";
import { t } from "../i18n";
import { LoadState } from "../components/LoadState";

/** Connecting GitHub, read-only: the owner's own app id, sign-in with a code typed at github.com, then reads that become attachments. */
export function GitHub() {
  const [status, { mutate, refetch }] = createResource(() => api.githubStatus().catch(() => null));
  const [client, setClient] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [said, setSaid] = createSignal("");
  const [bad, setBad] = createSignal("");
  const [repo, setRepo] = createSignal("");
  const [what, setWhat] = createSignal("");
  const [got, setGot] = createSignal<{ title: string; text: string; attachment?: AttachmentSummary } | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(timer));

  const fail = (e: unknown) => setBad(e instanceof ApiError ? e.message : t("gh.error"));
  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setSaid("");
    setBad("");
    try {
      await fn();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };
  // the service does not wait on GitHub by itself: this page asks again when GitHub allows the next question
  const poll = async () => {
    try {
      const p = await api.githubPoll();
      if (p.done) {
        setSaid(p.message);
        await refetch();
        return;
      }
      timer = setTimeout(() => void poll(), Math.max(1, p.next_poll_in ?? p.interval ?? 5) * 1000);
    } catch (e) {
      fail(e);
      await refetch();
    }
  };
  const start = () => run(async () => {
    await api.githubStart();
    await refetch();
    timer = setTimeout(() => void poll(), Math.max(1, status()?.pending?.next_poll_in ?? 5) * 1000);
  });
  const read = (attach: boolean) => run(async () => {
    setGot(null);
    const [owner, name] = repo().trim().split("/");
    const w = what().trim();
    const number = /^#?\d+$/.test(w) ? Number(w.replace("#", "")) : null;
    const r = number !== null
      ? await api.githubRead({ action: "issue", owner, repo: name, number, attach })
      : await api.githubRead({ action: "file", owner, repo: name, path: w, attach });
    const said = (r.comments ?? []).map((c) => `\n\n${c.author ?? ""}: ${c.body ?? ""}`).join("");
    setGot(r.issue
      ? { title: `#${r.issue.number ?? ""} ${r.issue.title ?? ""}`, text: `${r.issue.body ?? ""}${said}`, attachment: r.attachment }
      : { title: String(r.file?.path ?? w), text: String(r.file?.text ?? ""), attachment: r.attachment });
    if (r.attachment) {
      try { sessionStorage.setItem("kk-chat-attach", JSON.stringify(r.attachment)); } catch { /* the chat then opens without it */ }
    }
  });

  return (
    <section class="card stack" id="github" aria-labelledby="h-github">
      <h2 id="h-github">{t("gh.title")}</h2>
      <p class="muted">{t("gh.lead")}</p>
      <Show when={status()} fallback={<LoadState loading={status.loading} retry={() => void refetch()} />}>
        {(s) => (
          <Show when={s().available} fallback={<p class="muted">{s().problem || t("gh.unavailable")}</p>}>
            <Show when={!s().connected}>
              <form class="stack" onSubmit={(e) => { e.preventDefault(); void run(async () => { mutate(await api.githubClient(client().trim())); setClient(""); setSaid(t("gh.clientSaved")); }); }}>
                <label for="gh-client">{t(s().client_set ? "gh.clientSet" : "gh.client")}</label>
                <input id="gh-client" value={client()} maxlength="40" spellcheck={false} autocomplete="off" placeholder={s().client_id ?? "Iv23…"} onInput={(e) => setClient(e.currentTarget.value)} />
                <div class="actions">
                  <button type="submit" disabled={busy() || client().trim().length === 0}>{t("gh.saveClient")}</button>
                  <Show when={s().client_set && !s().pending}><button type="button" class="primary" disabled={busy()} onClick={() => void start()}>{t("gh.connect")}</button></Show>
                </div>
              </form>
              <Show when={s().pending}>
                {(p) => (
                  <div class="stack" id="gh-pending" role="status">
                    <p>{t("gh.enter")} <a href={p().verification_uri} target="_blank" rel="noopener noreferrer">{p().verification_uri}</a></p>
                    <p><code class="gh-code" id="gh-code">{p().user_code}</code></p>
                    <p class="muted">{t("gh.waiting")}</p>
                  </div>
                )}
              </Show>
            </Show>
            <Show when={s().connected}>
              <p id="gh-account"><strong>{t("gh.connected", { login: s().account?.login ?? "" })}</strong> <span class="muted">{t("gh.readOnly")}</span></p>
              <form class="stack" onSubmit={(e) => { e.preventDefault(); void read(false); }}>
                <label for="gh-repo">{t("gh.repo")}</label>
                <input id="gh-repo" value={repo()} placeholder="owner/repository" spellcheck={false} autocomplete="off" onInput={(e) => setRepo(e.currentTarget.value)} />
                <label for="gh-what">{t("gh.what")}</label>
                <input id="gh-what" value={what()} placeholder={t("gh.whatHint")} spellcheck={false} autocomplete="off" onInput={(e) => setWhat(e.currentTarget.value)} />
                <div class="actions">
                  <button type="submit" disabled={busy() || !/^[^/\s]+\/[^/\s]+$/.test(repo().trim()) || what().trim().length === 0}>{t("gh.read")}</button>
                  <button type="button" class="primary" disabled={busy() || !/^[^/\s]+\/[^/\s]+$/.test(repo().trim()) || what().trim().length === 0} onClick={() => void read(true)}>{t("gh.attach")}</button>
                  <button type="button" class="linklike" disabled={busy()} onClick={() => void run(async () => { const r = await api.githubSignOut(); setSaid(`${r.message} ${r.revoke ?? ""}`); setGot(null); await refetch(); })}>{t("gh.signOut")}</button>
                </div>
              </form>
              <Show when={got()}>
                {(g) => (
                  <div class="stack" id="gh-got">
                    <p class="muted">{t("gh.untrusted")}</p>
                    <p><strong>{g().title}</strong></p>
                    <pre class="wrap gh-text">{g().text}</pre>
                    <Show when={g().attachment}>
                      {(a) => <p class="ok" role="status">{t("gh.attached", { secrets: a().flagged?.secret ?? 0, orders: a().flagged?.instruction_like ?? 0 })} <a href="#/chat">{t("gh.openChat")}</a></p>}
                    </Show>
                  </div>
                )}
              </Show>
            </Show>
          </Show>
        )}
      </Show>
      <Show when={said()}><p role="status" class="ok">{said()}</p></Show>
      <Show when={bad()}><p role="alert" class="bad">{bad()}</p></Show>
    </section>
  );
}
