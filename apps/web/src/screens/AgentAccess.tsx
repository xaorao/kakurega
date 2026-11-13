import { Lead } from "../components/Lead";
import { LoadState } from "../components/LoadState";
import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api, ApiError } from "../api";
import { t, type Key } from "../i18n";

const toolLabel = (tool: string) => {
  const key = `agent.tool.${tool}` as Key;
  const text = t(key);
  return text === key ? tool : text;
};

export function AgentAccess() {
  const [access, { refetch }] = createResource(() => api.agentAccess().catch(() => null));
  const [tool, setTool] = createSignal("start_review");
  const [uses, setUses] = createSignal("3");
  const [minutes, setMinutes] = createSignal("60");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const timer = setInterval(() => void refetch(), 15000);
  onCleanup(() => clearInterval(timer));
  const minutesLeft = (l: { expires_at: number }, now: number) => Math.max(0, Math.ceil((l.expires_at - now) / 60));
  const whole = (v: string) => (/^\d{1,5}$/.test(v.trim()) ? Number(v) : NaN);
  const grant = async (e: Event) => {
    e.preventDefault();
    setError("");
    const n = whole(uses());
    const m = whole(minutes());
    if (!Number.isFinite(n) || !Number.isFinite(m)) {
      setError(t("agent.error.numbers"));
      return;
    }
    setBusy(true);
    try {
      await api.agentGrant(tool(), n, m);
      await refetch();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("agent.error.failed"));
    } finally {
      setBusy(false);
    }
  };
  const revoke = async (which?: string) => {
    setError("");
    try {
      await api.agentRevoke(which);
      await refetch();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("agent.error.failed"));
    }
  };
  return (
    <section class="stack wide" aria-labelledby="h-agent">
      <h1 id="h-agent">{t("agent.title")}</h1>
      <Lead text={t("agent.lead")} />
      <div class="card stack">
        <p>{t("agent.how")}</p>
        <p><code>kaku mcp --leased</code></p>
        <p class="muted">{t("agent.limits")}</p>
      </div>
      <div class="card stack" role="region" aria-label={t("agent.active")}>
        <h2>{t("agent.active")}</h2>
        <Show when={access()} fallback={<LoadState loading={access.loading} retry={() => void refetch()} />}>
          {(a) => (
            <Show when={a().leases.length > 0} fallback={<p role="status">{t("agent.none")}</p>}>
              <ul class="plain stack">
                <For each={a().leases}>
                  {(l) => (
                    <li class="actions">
                      <span><strong>{toolLabel(l.tool)}</strong> · {t("agent.left", { uses: l.remaining, minutes: minutesLeft(l, a().now) })}</span>
                      <button type="button" onClick={() => void revoke(l.tool)}>{t("agent.revoke")}</button>
                    </li>
                  )}
                </For>
              </ul>
              <p><button type="button" onClick={() => void revoke()}>{t("agent.revokeAll")}</button></p>
            </Show>
          )}
        </Show>
      </div>
      <form class="card stack" onSubmit={grant} aria-labelledby="h-grant">
        <h2 id="h-grant">{t("agent.grantTitle")}</h2>
        <label for="agent-tool">{t("agent.tool")}</label>
        <select id="agent-tool" value={tool()} onChange={(e) => setTool(e.currentTarget.value)}>
          <option value="start_review">{toolLabel("start_review")}</option>
          <option value="request_approval">{toolLabel("request_approval")}</option>
        </select>
        <label for="agent-uses">{t("agent.uses")}</label>
        <input id="agent-uses" class="short" inputmode="numeric" value={uses()} onInput={(e) => setUses(e.currentTarget.value)} />
        <label for="agent-minutes">{t("agent.minutes")}</label>
        <input id="agent-minutes" class="short" inputmode="numeric" value={minutes()} onInput={(e) => setMinutes(e.currentTarget.value)} />
        <div class="actions">
          <button class="primary" type="submit" disabled={busy()}>{t("agent.grant")}</button>
          <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
        </div>
      </form>
    </section>
  );
}
