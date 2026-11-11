import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type ProjectRow } from "../api";
import { t } from "../i18n";

const leave = (key: string, value: string) => { try { sessionStorage.setItem(key, value); } catch { /* the screen then opens without it */ } };

/** The owner's projects: each with the folder of its documents and what belongs to it, and a way to make a new one. */
export function Projects() {
  const [list, { mutate }] = createResource(() => api.projects().then((r) => r.projects));
  const [name, setName] = createSignal("");
  const [folder, setFolder] = createSignal("");
  const [said, setSaid] = createSignal("");
  const [bad, setBad] = createSignal("");
  const change = async (body: { name?: string; folder?: string; remove?: string }, ok: string) => {
    setSaid("");
    setBad("");
    try {
      const r = await api.projectChange(body);
      mutate(r.projects);
      setSaid(r.kept ? `${ok} ${r.kept}` : ok);
      setName("");
      setFolder("");
    } catch (e) {
      setBad(e instanceof ApiError ? e.message : t("proj.error"));
    }
  };
  const ask = (p: ProjectRow) => { leave("kk-docs-folder", p.folder ?? ""); location.hash = "#/docs"; };
  const chat = (p: ProjectRow) => { leave("kk-chat-project", p.name); location.hash = "#/chat"; };
  return (
    <section class="stack wide" aria-labelledby="h-proj">
      <h1 id="h-proj">{t("proj.title")}</h1>
      <p class="lead">{t("proj.lead")}</p>
      <form class="card stack" onSubmit={(e) => { e.preventDefault(); void change({ name: name().trim(), ...(folder().trim() ? { folder: folder().trim() } : {}) }, t("proj.saved")); }}>
        <h2>{t("proj.new")}</h2>
        <label for="proj-name">{t("proj.name")}</label>
        <input id="proj-name" value={name()} maxlength="80" placeholder={t("proj.nameHint")} onInput={(e) => setName(e.currentTarget.value)} />
        <label for="proj-folder">{t("proj.folder")}</label>
        <input id="proj-folder" value={folder()} placeholder={t("proj.folderHint")} spellcheck={false} autocomplete="off" onInput={(e) => setFolder(e.currentTarget.value)} />
        <div class="actions"><button class="primary" type="submit" disabled={name().trim().length === 0}>{t("proj.save")}</button></div>
        <Show when={said()}><p role="status" class="ok">{said()}</p></Show>
        <Show when={bad()}><p role="alert" class="bad">{bad()}</p></Show>
      </form>
      <Show when={list()} fallback={<p class="muted" role={list.error ? "alert" : "status"}>{list.error ? t("proj.error") : t("review.loading")}</p>}>
        {(rows) => (
          <ul class="plain stack" id="proj-list" aria-label={t("proj.title")}>
            <For each={rows()} fallback={<li class="card muted">{t("proj.none")}</li>}>
              {(p) => (
                <li class="card stack">
                  <h2>{p.name}</h2>
                  <ul class="plain muted">
                    <li>{p.folder ? (p.folder_available ? t("proj.docsIn", { folder: p.folder }) : t("proj.docsGone", { folder: p.folder })) : t("proj.noFolder")}</li>
                    <li>{t("proj.chats", { n: p.conversations })}</li>
                    <Show when={p.notes_only}><li>{t("proj.notesOnly")}</li></Show>
                  </ul>
                  <div class="actions">
                    <button type="button" onClick={() => chat(p)}>{t("proj.chat")}</button>
                    <Show when={p.folder && p.folder_available}><button type="button" onClick={() => ask(p)}>{t("proj.ask")}</button></Show>
                    <a class="button" href="#/instructions">{t("proj.instructions")}</a>
                    <a class="button" href="#/memory">{t("proj.notes")}</a>
                    <Show when={p.listed}><button type="button" class="linklike" onClick={() => void change({ remove: p.name }, t("proj.removed"))}>{t("proj.remove")} <span class="sr">{p.name}</span></button></Show>
                  </div>
                </li>
              )}
            </For>
          </ul>
        )}
      </Show>
    </section>
  );
}
