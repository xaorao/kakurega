import { LoadState } from "../components/LoadState";
import { createResource, createSignal, For, onCleanup, onMount, Show } from "solid-js";
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
  const [knowing, setKnowing] = createSignal<Record<string, boolean>>({});
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
              <label for={`mcp-${props.s.name}-${tool.name}`}><strong>{tool.name}</strong></label>{" "}
              <select
                id={`mcp-${props.s.name}-${tool.name}`}
                aria-label={t("mcp.mode", { name: tool.name })}
                value={tool.mode}
                disabled={tool.mode === "off" && tool.effect !== "read_only" && !knowing()[tool.name]}
                onChange={(e) => void run(async () => { await api.mcpTool(props.s.name, tool.name, e.currentTarget.value as "off" | "allow" | "ask", tool.pin, !!knowing()[tool.name]); })}
              >
                <option value="off">{t("mcp.mode.off")}</option>
                <option value="ask">{t("mcp.mode.ask")}</option>
                <option value="allow">{t("mcp.mode.allow")}</option>
              </select>
              <Show when={tool.description}><p class="muted">{tool.description}</p></Show>
              <p class={tool.effect === "read_only" ? "muted" : "bad"}>{t(`mcp.effect.${tool.effect}` as never)}</p>
              <Show when={tool.schema}><details><summary>{t("mcp.schema")}</summary><pre class="plain-pre">{tool.schema}</pre></details></Show>
              <Show when={tool.effect !== "read_only" && tool.mode === "off"}>
                <label for={`mcp-k-${props.s.name}-${tool.name}`}>
                  <input id={`mcp-k-${props.s.name}-${tool.name}`} type="checkbox" checked={!!knowing()[tool.name]} onChange={(e) => setKnowing({ ...knowing(), [tool.name]: e.currentTarget.checked })} /> {t("mcp.knowing")}
                </label>
              </Show>
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

/** Calls a coding task is waiting to make, with their arguments, until the owner says yes or no. */
export function McpPending() {
  const [calls, setCalls] = createSignal<{ id: string; server: string; tool: string; args: string }[]>([]);
  const [problem, setProblem] = createSignal("");
  const poll = async () => {
    try { setCalls((await api.mcpPending()).pending); } catch { /* the page keeps its last answer */ }
  };
  onMount(() => {
    void poll();
    const timer = setInterval(() => void poll(), 1500);
    onCleanup(() => clearInterval(timer));
  });
  const decide = async (id: string, approve: boolean) => {
    setProblem("");
    try { await api.mcpDecide(id, approve); await poll(); } catch (e) { setProblem(e instanceof ApiError ? e.message : t("mcp.error")); }
  };
  return (
    <Show when={calls().length > 0}>
      <section class="card stack" role="alert" aria-labelledby="h-mcp-pending">
        <h2 id="h-mcp-pending">{t("mcp.pending.heading")}</h2>
        <p class="muted">{t("mcp.pending.note")}</p>
        <ul class="plain stack">
          <For each={calls()}>
            {(c) => (
              <li class="stack">
                <p><code>{t("mcp.pending.call", { server: c.server, tool: c.tool, args: c.args })}</code></p>
                <div class="actions">
                  <button class="primary" type="button" onClick={() => void decide(c.id, true)}>{t("mcp.pending.approve")}</button>
                  <button type="button" onClick={() => void decide(c.id, false)}>{t("mcp.pending.deny")}</button>
                </div>
              </li>
            )}
          </For>
        </ul>
        <Show when={problem()}><span role="alert" class="bad">{problem()}</span></Show>
      </section>
    </Show>
  );
}

export function Mcp() {
  const [data, { refetch }] = createResource(() => api.mcp().catch(() => null));
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
      <McpPending />
      <Show when={status()}><p role="status" class="ok">{status()}</p></Show>
      <Show when={data()} fallback={<LoadState loading={data.loading} retry={() => void refetch()} />}>
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
