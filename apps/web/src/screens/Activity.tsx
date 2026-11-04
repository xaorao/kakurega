import { createResource, For, Show } from "solid-js";
import { api } from "../api";
import { rich, t, type Key } from "../i18n";

function label(kind: string): string {
  const key = `log.kind.${kind}` as Key;
  const text = t(key);
  return text === key ? kind : text;
}

export function Activity() {
  const [log] = createResource(() => api.log());
  return (
    <section class="stack wide" aria-labelledby="h-act">
      <h1 id="h-act">{t("act.title")}</h1>
      <p class="lead">{t("act.lead")}</p>
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
