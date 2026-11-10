import { createResource, createSignal, For, Show } from "solid-js";
import { api, ApiError, type McpServer } from "../api";
import { t } from "../i18n";

const lines = (text: string) => text.split("\n").map((l) => l.trim()).filter(Boolean);

function envFrom(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const l of lines(text)) {
    const i = l.indexOf("=");
    if (i > 0) out[l.slice(0, i).trim()] = l.slice(i + 1);
  }
  return out;
}

function Server(props: { s: McpServer; onChanged: (message: string) => void }) {
  const [problem, setProblem] = createSignal("");
  const run = async (fn: () => Promise<string | void>) => {
    setProblem("");
    try {
      const message = await fn();
      props.onChanged(message ?? "");
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mcp.error"));
    }
  };
  return (
    <li class="card stack">
      <h3>{props.s.name}</h3>
      <p class="muted"><code>{props.s.program}</code> {props.s.args.join(" ")}</p>
      <Show when={props.s.env_names?.length}><p class="muted">{t("mcp.envNames", { list: (props.s.env_names ?? []).join(", ") })}</p></Show>
      <Show when={props.s.tools.length === 0}><p class="muted">{t("mcp.noTools")}</p></Show>
      <ul class="plain stack">
        <For each={props.s.tools}>
          {(tool) => (
            <li class="stack">
              <label for={`mcp-${props.s.name}-${tool.name}`}>
                <input
                  id={`mcp-${props.s.name}-${tool.name}`}
                  type="checkbox"
                  checked={tool.mode === "allow"}
                  onChange={(e) => void run(async () => { await api.mcpTool(props.s.name, tool.name, e.currentTarget.checked ? "allow" : "off", tool.pin); })}
                />{" "}
                <strong>{tool.name}</strong> {tool.mode === "allow" ? t("mcp.allowed") : t("mcp.off")}
              </label>
              <Show when={tool.description}><p class="muted">{tool.description}</p></Show>
            </li>
          )}
        </For>
      </ul>
      <div class="actions">
        <button type="button" onClick={() => void run(async () => {
          const r = await api.mcpDiscover(props.s.name);
          return t("mcp.checked", { n: r.tools }) + (r.changed.length ? " " + t("mcp.changedTools", { list: r.changed.join(", ") }) : "");
        })}>{t("mcp.check")}</button>
        <button type="button" onClick={() => void run(async () => { await api.mcpRemove(props.s.name); return t("mcp.removed"); })}>{t("mcp.remove")}</button>
      </div>
      <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
    </li>
  );
}

export function Mcp() {
  const [data, { refetch }] = createResource(() => api.mcp());
  const [status, setStatus] = createSignal("");
  const [name, setName] = createSignal("");
  const [program, setProgram] = createSignal("");
  const [args, setArgs] = createSignal("");
  const [env, setEnv] = createSignal("");
  const [problem, setProblem] = createSignal("");
  const changed = (message: string) => { if (message) setStatus(message); void refetch(); };
  const add = async () => {
    setProblem("");
    try {
      await api.mcpAdd({ name: name().trim(), program: program().trim(), args: lines(args()), env: envFrom(env()) });
      setName(""); setProgram(""); setArgs(""); setEnv("");
      changed(t("mcp.saved"));
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : t("mcp.error"));
    }
  };
  return (
    <div class="card stack" aria-labelledby="h-mcp">
      <h2 id="h-mcp">{t("mcp.heading")}</h2>
      <p class="muted">{t("mcp.lead")}</p>
      <p class="muted">{t("mcp.warn")}</p>
      <Show when={status()}><p role="status" class="ok">{status()}</p></Show>
      <Show when={data()} fallback={<p class="muted">{t("review.loading")}</p>}>
        {(d) => (
          <>
            <Show when={d().servers.length === 0}><p class="muted">{t("mcp.none")}</p></Show>
            <ul class="plain stack"><For each={d().servers}>{(s) => <Server s={s} onChanged={changed} />}</For></ul>
            <label for="mcp-name">{t("mcp.name")}</label>
            <input id="mcp-name" value={name()} maxlength="64" onInput={(e) => setName(e.currentTarget.value)} />
            <label for="mcp-program">{t("mcp.program")}</label>
            <input id="mcp-program" value={program()} onInput={(e) => setProgram(e.currentTarget.value)} />
            <label for="mcp-args">{t("mcp.args")}</label>
            <textarea id="mcp-args" rows="2" value={args()} onInput={(e) => setArgs(e.currentTarget.value)} />
            <label for="mcp-env">{t("mcp.env")}</label>
            <textarea id="mcp-env" rows="2" value={env()} onInput={(e) => setEnv(e.currentTarget.value)} />
            <p class="muted">{t("mcp.envHint")}</p>
            <div class="actions"><button class="primary" type="button" disabled={!name().trim() || !program().trim()} onClick={() => void add()}>{t("mcp.add")}</button></div>
            <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
          </>
        )}
      </Show>
    </div>
  );
}
