import { Lead } from "../components/Lead";
import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError } from "../api";
import { LoadState } from "../components/LoadState";
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

/** The switch that makes one project answer only from its own notes and attached files. */
function NotesOnly(props: { project: string; on: boolean; onChanged: (message: string) => void }) {
  const [problem, setProblem] = createSignal("");
  const id = () => `ins-no-${props.project}`;
  const change = async (on: boolean) => {
    setProblem("");
    try {
      await api.setNotesOnly(props.project, on);
      props.onChanged(t(on ? "ins.notesOnly.on" : "ins.notesOnly.off", { name: props.project }));
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("ins.error"));
    }
  };
  return (
    <div class="stack">
      <label for={id()}>
        <input id={id()} type="checkbox" checked={props.on} onChange={(e) => void change(e.currentTarget.checked)} /> {t("ins.notesOnly.label")}
      </label>
      <p class="muted">{t("ins.notesOnly.hint")}</p>
      <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
    </div>
  );
}

export function Instructions() {
  const [data, { refetch }] = createResource(() => api.instructions().catch(() => null));
  // the switches are shown only once their state is known: a setting that could not be read must not look like "off"
  const [settings, { refetch: refetchSettings }] = createResource(() => api.projectSettings().catch(() => null));
  const retry = () => {
    if (!data()) void refetch();
    if (!settings()) void refetchSettings();
  };
  const notesOnly = () => new Set((settings()?.projects ?? []).map((p) => p.project));
  // a project stays listed after its switch is turned off, so the confirmation is not removed from under the person
  const [touched, setTouched] = createSignal<string[]>([]);
  const [changed, setChanged] = createSignal("");
  const names = () => {
    const all = new Set<string>([...(data()?.projects ?? []).map((p) => p.project), ...notesOnly(), ...touched()]);
    return [...all].sort();
  };
  const [newProject, setNewProject] = createSignal("");
  return (
    <section class="stack wide" aria-labelledby="h-ins">
      <h1 id="h-ins">{t("ins.title")}</h1>
      <Lead text={t("ins.lead")} />
      <p class="muted">{t("ins.limits")}</p>
      <Show when={settings() ? data() : null} fallback={<LoadState loading={data.loading || settings.loading} retry={retry} />}>
        {(d) => (
          <>
            <div class="card stack">
              <h2>{t("ins.global.heading")}</h2>
              <Editor id="ins-global" label={t("ins.global.label")} initial={d().global?.text ?? ""} max={d().max_chars} onSaved={() => void refetch()} />
            </div>
            <div class="card stack">
              <h2>{t("ins.projects.heading")}</h2>
              <p class="muted">{t("ins.projects.lead")}</p>
              <Show when={changed()}><p role="status" class="ok">{changed()}</p></Show>
              <ul class="plain stack">
                <For each={names()}>
                  {(name) => (
                    <li class="card stack">
                      <Editor id={`ins-p-${name}`} label={t("ins.project.label", { name })} initial={d().projects.find((p) => p.project === name)?.text ?? ""} project={name} max={d().max_chars} onSaved={() => void refetch()} />
                      <NotesOnly project={name} on={notesOnly().has(name)} onChanged={(m) => { setTouched([...touched(), name]); setChanged(m); void refetchSettings(); }} />
                    </li>
                  )}
                </For>
              </ul>
              <label for="ins-new">{t("ins.project.new")}</label>
              <input id="ins-new" value={newProject()} maxlength="80" placeholder={t("ins.project.placeholder")} onInput={(e) => setNewProject(e.currentTarget.value)} />
              <Show when={newProject().trim() !== ""}>
                <NotesOnly project={newProject().trim()} on={notesOnly().has(newProject().trim())} onChanged={(m) => { setTouched([...touched(), newProject().trim()]); setNewProject(""); setChanged(m); void refetchSettings(); }} />
                <Editor id="ins-new-text" label={t("ins.project.label", { name: newProject().trim() })} initial="" project={newProject().trim()} max={d().max_chars} onSaved={() => { setNewProject(""); void refetch(); }} />
              </Show>
            </div>
            <p class="muted"><a class="line-link" href="#/skills">{t("skills.moved")}</a></p>
          </>
        )}
      </Show>
    </section>
  );
}
