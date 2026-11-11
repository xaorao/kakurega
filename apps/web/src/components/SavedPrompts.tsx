import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError } from "../api";
import { t } from "../i18n";

/** The owner's saved prompts under the chat box: one click puts a prompt into the box, and what is in the box can be saved. */
export function SavedPrompts(props: { text: () => string; onUse: (text: string) => void }) {
  const [list, { mutate }] = createResource(() => api.prompts().then((r) => r.prompts).catch(() => []));
  const [name, setName] = createSignal("");
  const [said, setSaid] = createSignal("");
  const [bad, setBad] = createSignal("");
  const change = async (body: { name?: string; text?: string; remove?: string }, ok: string) => {
    setSaid("");
    setBad("");
    try {
      const r = await api.promptChange(body);
      mutate(r.prompts);
      setSaid(r.changed ? ok : "");
      setName("");
    } catch (e) {
      setBad(e instanceof ApiError ? e.message : t("prompts.error"));
    }
  };
  return (
    <details class="saved-prompts" id="saved-prompts">
      <summary>{t("prompts.title", { n: list()?.length ?? 0 })}</summary>
      <ul class="plain stack">
        <For each={list()} fallback={<li class="muted">{t("prompts.none")}</li>}>
          {(p) => (
            <li class="actions">
              <button type="button" class="prompt-use" title={p.text} onClick={() => props.onUse(p.text)}>{p.name}</button>
              <button type="button" class="linklike" onClick={() => void change({ remove: p.name }, t("prompts.removed"))}>{t("set.pii.remove")} <span class="sr">{p.name}</span></button>
            </li>
          )}
        </For>
      </ul>
      <form class="actions" onSubmit={(e) => { e.preventDefault(); void change({ name: name(), text: props.text() }, t("prompts.saved")); }}>
        <label class="sr" for="prompt-name">{t("prompts.name")}</label>
        <input id="prompt-name" value={name()} maxlength="60" placeholder={t("prompts.name")} onInput={(e) => setName(e.currentTarget.value)} />
        <button type="submit" disabled={name().trim().length === 0 || props.text().trim().length === 0}>{t("prompts.save")}</button>
      </form>
      <Show when={said()}><p role="status" class="ok">{said()}</p></Show>
      <Show when={bad()}><p role="alert" class="bad">{bad()}</p></Show>
    </details>
  );
}
