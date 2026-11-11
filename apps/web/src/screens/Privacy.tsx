import { For, Show, createResource, onCleanup } from "solid-js";
import { api, type OutboundRow } from "../api";
import { t, type Key } from "../i18n";
import { modelKey } from "../privacy";

function Card(props: { id: string; title: Key; children: unknown }) {
  return (
    <section class="card stack" aria-labelledby={`pv-${props.id}`}>
      <h2 id={`pv-${props.id}`}>{t(props.title)}</h2>
      {props.children as never}
    </section>
  );
}

/** What left this computer and what was held back, newest first, refreshed while the page is open. */
function Outbound() {
  const [list, { refetch }] = createResource(() => api.outbound().catch(() => null));
  const timer = setInterval(() => void refetch(), 4000);
  const changed = () => void refetch();
  window.addEventListener("kk-changed", changed);
  onCleanup(() => { clearInterval(timer); window.removeEventListener("kk-changed", changed); });
  const line = (r: OutboundRow) => {
    const d = r.detail;
    switch (r.what) {
      case "model": return t("out.model", { to: r.to, chars: Number(d.chars ?? 0), n: Number(d.placeholders ?? 0) });
      case "search": return t("out.search", { n: Number(d.results ?? 0) });
      case "pages": return t("out.pages", { n: Number(d.pages ?? 0) });
      case "tool": return t("out.tool", { to: r.to });
      case "release": return t("out.release", { to: r.to });
      case "review": return t("out.review", { to: r.to, calls: Number(d.calls ?? 0), tokens: Number(d.tokens ?? 0) });
      default: return t("out.held", { where: r.to, secrets: Number(d.secrets ?? 0), orders: Number(d.instructions ?? 0) });
    }
  };
  return (
    <section class="card stack" id="outbound" aria-labelledby="h-outbound">
      <h2 id="h-outbound">{t("out.title")}</h2>
      <Show when={list()} fallback={<p class="muted" role="status">{list.loading ? t("inbox.loading") : t("privacy.unavailable")}</p>}>
        {(o) => (
          <>
            <p class="muted">{o().note}</p>
            <ul class="plain stack" aria-live="polite">
              <For each={o().rows} fallback={<li class="muted">{t("out.none")}</li>}>
                {(r) => <li class={r.what === "held" ? "out-row held" : "out-row"}><time class="muted">{new Date(r.at * 1000).toLocaleTimeString()}</time> <span class={r.what === "held" ? "chip ok" : "chip"}>{t(`out.kind.${r.what}`)}</span> {line(r)}</li>}
              </For>
            </ul>
          </>
        )}
      </Show>
    </section>
  );
}

export function Privacy() {
  const [facts] = createResource(() => api.privacy().catch(() => null));
  return (
    <section class="stack wide" aria-labelledby="h-privacy">
      <h1 id="h-privacy">{t("privacy.title")}</h1>
      <p class="lead">{t("privacy.lead")}</p>
      <Show when={facts()} fallback={<p class="muted" role="status">{facts.loading ? t("inbox.loading") : t("privacy.unavailable")}</p>}>
        {(f) => (
          <>
            <Outbound />
            <Card id="break" title="break.title">
              <p>{t("break.card")}</p>
              <p><a class="button" href="#/break">{t("break.open")}</a></p>
            </Card>
            <Card id="model" title="privacy.model.title">
              <p>{t(modelKey(f().model), { name: f().model.name ?? f().model.id, host: f().model.host ?? "" })}</p>
              <p class="muted">{t("privacy.model.sent")}</p>
            </Card>
            <Card id="notes" title="privacy.notes.title">
              <p>{t("privacy.notes.body", { n: f().notes.count })}</p>
            </Card>
            <Card id="reviews" title="privacy.reviews.title">
              <p>{t("privacy.reviews.body")}</p>
              <p class="muted">{t(f().approvals.signing_key_enrolled ? "privacy.reviews.signed" : "privacy.reviews.unsigned")}</p>
            </Card>
            <Card id="web" title="privacy.web.title">
              <p>{f().search.on ? t("privacy.web.on", { host: f().search.host ?? "" }) : t("privacy.web.off")}</p>
            </Card>
            <Card id="telegram" title="privacy.tg.title">
              <p>{t(f().telegram.on ? "privacy.tg.on" : "privacy.tg.off")}</p>
            </Card>
            <Card id="extension" title="privacy.ext.title">
              <p>{t(f().extension.on ? "privacy.ext.on" : "privacy.ext.off")}</p>
            </Card>
            <Card id="agents" title="privacy.agents.title">
              <p>{t("privacy.agents.body")}</p>
              <p>{f().agents.tools_open.length > 0 ? t("privacy.agents.open", { tools: f().agents.tools_open.join(", ") }) : t("privacy.agents.none")}</p>
            </Card>
            <Card id="updates" title="privacy.updates.title">
              <p>{t("privacy.updates.body")}</p>
            </Card>
            <Card id="limits" title="privacy.limits.title">
              <ul class="stack">
                <For each={["privacy.limits.1", "privacy.limits.2", "privacy.limits.3", "privacy.limits.4"] as Key[]}>{(k) => <li>{t(k)}</li>}</For>
              </ul>
            </Card>
          </>
        )}
      </Show>
    </section>
  );
}
