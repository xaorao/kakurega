import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError } from "../api";
import { t } from "../i18n";
import { fillPrompt, variablesOf, type PromptVar } from "../promptVars";

/** The owner's saved prompts under the chat box: one click puts a prompt into the box, and what is in the box can be saved. */
export function SavedPrompts(props: { text: () => string; onUse: (text: string) => void }) {
  const [list, { mutate }] = createResource(() => api.prompts().then((r) => r.prompts).catch(() => []));
  const [name, setName] = createSignal("");
  const [said, setSaid] = createSignal("");
  const [bad, setBad] = createSignal("");
  // a prompt with fields to fill: its text, the fields, and what has been typed so far
  const [filling, setFilling] = createSignal<{ name: string; text: string; vars: PromptVar[] } | null>(null);
  const [values, setValues] = createSignal<Record<string, string>>({});
  const use = (name: string, text: string) => {
    setBad("");
    setSaid("");
    const vars = variablesOf(text);
    if (vars.length === 0) {
      setFilling(null);
      const r = fillPrompt(text, {}, new Date());
      props.onUse("text" in r ? r.text : text);
      return;
    }
    setValues({});
    setFilling({ name, text, vars });
  };
  const put = () => {
    const f = filling();
    if (!f) return;
    const r = fillPrompt(f.text, values(), new Date());
    if ("missing" in r) {
      setBad(t(r.missing.length === 1 ? "prompts.vars.missing.one" : "prompts.vars.missing", { names: r.missing.join(", ") }));
      return;
    }
    setFilling(null);
    props.onUse(r.text);
  };
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
              <button type="button" class="prompt-use" title={p.text} onClick={() => use(p.name, p.text)}>{p.name}</button>
              <button type="button" class="linklike" onClick={() => void change({ remove: p.name }, t("prompts.removed"))}>{t("set.pii.remove")} <span class="sr">{p.name}</span></button>
            </li>
          )}
        </For>
      </ul>
      <p class="muted">{t("prompts.vars.hint")}</p>
      <Show when={filling()}>
        {(f) => (
          <form class="card stack prompt-vars" aria-labelledby="prompt-vars-h" onSubmit={(e) => { e.preventDefault(); put(); }}>
            <p id="prompt-vars-h"><strong>{f().name}</strong> <span class="muted">{t("prompts.vars.lead")}</span></p>
            <For each={f().vars}>
              {(v, i) => (
                <>
                  <label for={`prompt-var-${i()}`}>{v.name}</label>
                  <input id={`prompt-var-${i()}`} list={v.options.length ? `prompt-var-opts-${i()}` : undefined} value={Object.prototype.hasOwnProperty.call(values(), v.name) ? values()[v.name] : ""} maxlength="2000" onInput={(e) => { setBad(""); setValues({ ...values(), [v.name]: e.currentTarget.value }); }} />
                  <Show when={v.options.length > 0}>
                    <datalist id={`prompt-var-opts-${i()}`}><For each={v.options}>{(o) => <option value={o} />}</For></datalist>
                  </Show>
                </>
              )}
            </For>
            <div class="actions">
              <button class="primary" type="submit">{t("prompts.vars.put")}</button>
              <button type="button" onClick={() => { setBad(""); setFilling(null); }}>{t("prompts.vars.cancel")}</button>
            </div>
            <Show when={bad()}><p role="alert" class="bad">{bad()}</p></Show>
          </form>
        )}
      </Show>
      <form class="actions" onSubmit={(e) => { e.preventDefault(); void change({ name: name(), text: props.text() }, t("prompts.saved")); }}>
        <label class="sr" for="prompt-name">{t("prompts.name")}</label>
        <input id="prompt-name" value={name()} maxlength="60" placeholder={t("prompts.name")} onInput={(e) => setName(e.currentTarget.value)} />
        <button type="submit" disabled={name().trim().length === 0 || props.text().trim().length === 0}>{t("prompts.save")}</button>
      </form>
      <Show when={said()}><p role="status" class="ok">{said()}</p></Show>
      <Show when={bad() && !filling()}><p role="alert" class="bad">{bad()}</p></Show>
    </details>
  );
}
