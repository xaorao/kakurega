import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError } from "../api";
import { t } from "../i18n";

/** One box: the text, how much room is left, save and remove. An empty text removes the instruction. */
function Editor(props: { id: string; label: string; initial: string; project?: string; max: number; onSaved: () => void }) {
  const [text, setText] = createSignal(props.initial);
  const [status, setStatus] = createSignal("");
  const [problem, setProblem] = createSignal("");
  const save = async (value: string) => {
    setStatus("");
    setProblem("");
    try {
      await api.setInstruction(props.project, value);
      setStatus(value.trim() ? t("ins.saved") : t("ins.removed"));
      if (!value.trim()) setText("");
      props.onSaved();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("ins.error"));
    }
  };
  return (
    <div class="stack">
      <label for={props.id}>{props.label}</label>
      <textarea id={props.id} rows="5" maxlength={props.max} value={text()} onInput={(e) => { setText(e.currentTarget.value); setStatus(""); }} />
      <p class="muted">{t("ins.count", { n: text().length, max: props.max })}</p>
      <div class="actions">
        <button class="primary" type="button" disabled={text().trim() === props.initial.trim()} onClick={() => void save(text())}>{t("ins.save")}</button>
        <Show when={props.initial.trim() !== "" || text().trim() !== ""}>
          <button type="button" onClick={() => void save("")}>{t("ins.remove")}</button>
        </Show>
        <Show when={status()}><span role="status" class="ok">{status()}</span></Show>
        <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
      </div>
    </div>
  );
}

export function Instructions() {
  const [data, { refetch }] = createResource(() => api.instructions());
  const [newProject, setNewProject] = createSignal("");
  return (
    <section class="stack wide" aria-labelledby="h-ins">
      <h1 id="h-ins">{t("ins.title")}</h1>
      <p class="lead">{t("ins.lead")}</p>
      <p class="muted">{t("ins.limits")}</p>
      <Show when={data()} fallback={<p class="muted">{t("review.loading")}</p>}>
        {(d) => (
          <>
            <div class="card stack">
              <h2>{t("ins.global.heading")}</h2>
              <Editor id="ins-global" label={t("ins.global.label")} initial={d().global?.text ?? ""} max={d().max_chars} onSaved={() => void refetch()} />
            </div>
            <div class="card stack">
              <h2>{t("ins.projects.heading")}</h2>
              <p class="muted">{t("ins.projects.lead")}</p>
              <ul class="plain stack">
                <For each={d().projects}>
                  {(p) => (
                    <li class="card stack">
                      <Editor id={`ins-p-${p.project}`} label={t("ins.project.label", { name: p.project })} initial={p.text} project={p.project} max={d().max_chars} onSaved={() => void refetch()} />
                    </li>
                  )}
                </For>
              </ul>
              <label for="ins-new">{t("ins.project.new")}</label>
              <input id="ins-new" value={newProject()} maxlength="80" placeholder={t("mem.add.projectHint")} onInput={(e) => setNewProject(e.currentTarget.value)} />
              <Show when={newProject().trim() !== ""}>
                <Editor id="ins-new-text" label={t("ins.project.label", { name: newProject().trim() })} initial="" project={newProject().trim()} max={d().max_chars} onSaved={() => { setNewProject(""); void refetch(); }} />
              </Show>
            </div>
          </>
        )}
      </Show>
    </section>
  );
}
