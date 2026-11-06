import { createResource, createSignal, For, Show } from "solid-js";
import { api } from "../api";
import { rich, t, type Key } from "../i18n";

function label(kind: string): string {
  const key = `log.kind.${kind}` as Key;
  const text = t(key);
  return text === key ? kind : text;
}

const DAY = 86_400;
const day = (secs: number) => new Date(secs * 1000).toISOString().slice(0, 10);

/** What each model route answered over a range of days: replies, tokens in and out, and how fast. No prices: none are known here. */
function UsageCard() {
  const nowSecs = Math.floor(Date.now() / 1000);
  const [from, setFrom] = createSignal(day(nowSecs - 29 * DAY));
  const [to, setTo] = createSignal(day(nowSecs));
  const [problem, setProblem] = createSignal("");
  const range = () => {
    const a = Date.parse(from() + "T00:00:00Z") / 1000;
    const b = Date.parse(to() + "T00:00:00Z") / 1000 + DAY; // the last day is included
    return Number.isFinite(a) && Number.isFinite(b) ? ([a, b] as const) : null;
  };
  const [usage] = createResource(range, async (r) => {
    setProblem("");
    if (!r || r[0] >= r[1] || r[1] - r[0] > 366 * DAY) {
      setProblem(t("use.badRange"));
      return null;
    }
    try {
      return await api.usage(r[0], r[1]);
    } catch {
      setProblem(t("use.error"));
      return null;
    }
  });
  return (
    <section class="card stack" id="usage" aria-labelledby="h-use">
      <h2 id="h-use">{t("use.title")}</h2>
      <p class="muted">{t("use.lead")}</p>
      <div class="actions">
        <label for="use-from">{t("use.from")}</label>
        <input id="use-from" type="date" value={from()} onInput={(e) => setFrom(e.currentTarget.value)} />
        <label for="use-to">{t("use.to")}</label>
        <input id="use-to" type="date" value={to()} onInput={(e) => setTo(e.currentTarget.value)} />
      </div>
      <Show when={problem()}><p role="alert" class="bad">{problem()}</p></Show>
      <Show when={usage()}>
        {(u) => (
          <Show when={u().routes.length > 0} fallback={<p>{t("use.none")}</p>}>
            <table>
              <thead><tr><th>{t("use.col.route")}</th><th>{t("use.col.replies")}</th><th>{t("use.col.in")}</th><th>{t("use.col.out")}</th><th>{t("use.col.speed")}</th></tr></thead>
              <tbody>
                <For each={u().routes}>
                  {(r) => (
                    <tr>
                      <td>{r.route}{r.simulated ? ` (${t("chat.test")})` : ""}</td>
                      <td>{r.turns}</td>
                      <td>{r.prompt_tokens}{r.measured_turns < r.turns ? "*" : ""}</td>
                      <td>{r.reply_tokens}</td>
                      <td>{t("use.speed", { ms: r.avg_ms, tps: r.reply_tokens_per_second })}</td>
                    </tr>
                  )}
                </For>
                <tr><th scope="row">{t("use.total")}</th><td>{u().total.turns}</td><td>{u().total.prompt_tokens}</td><td>{u().total.reply_tokens}</td><td></td></tr>
              </tbody>
            </table>
            <p class="muted">{t("use.note")}</p>
          </Show>
        )}
      </Show>
    </section>
  );
}

export function Activity() {
  const [log] = createResource(() => api.log());
  return (
    <section class="stack wide" aria-labelledby="h-act">
      <h1 id="h-act">{t("act.title")}</h1>
      <p class="lead">{t("act.lead")}</p>
      <UsageCard />
      <Show when={log()} fallback={<p class="muted">{t("review.loading")}</p>}>
        {(l) => (
          <>
            <div class="card stack">
              <p>{t("act.count", { count: l().count })}</p>
              <p class="muted">{t("act.head")}: <code>{l().head ?? "-"}</code></p>
              <p class="muted">{t("act.key")}: <code>{l().public_key}</code></p>
              <p><a class="button" href="/api/log/export" download="kakurega-event-log.json">{t("act.download")}</a></p>
              <p class="muted">{rich("act.verify", { key: l().public_key })}</p>
            </div>
            <div class="card">
              <table>
                <thead><tr><th>{t("act.col.when")}</th><th>{t("act.col.event")}</th><th>{t("act.col.subject")}</th></tr></thead>
                <tbody>
                  <For each={l().recent} fallback={<tr><td colSpan={3} class="muted">{t("act.none")}</td></tr>}>
                    {(e) => <tr><td>{new Date(e.at * 1000).toLocaleString()}</td><td>{label(e.kind)}</td><td><code>{e.subject}</code></td></tr>}
                  </For>
                </tbody>
              </table>
            </div>
          </>
        )}
      </Show>
    </section>
  );
}
