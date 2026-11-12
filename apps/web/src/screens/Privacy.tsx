import { For, Show, createResource, onCleanup } from "solid-js";
import { api, type OutboundRow, type PrivacyFacts } from "../api";
import { t, type Key, locale } from "../i18n";
import { modelKey } from "../privacy";
import { whenToday } from "../when";

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
      case "pages": return t(Number(d.pages ?? 0) === 1 ? "out.pages.one" : "out.pages", { n: Number(d.pages ?? 0) });
      case "tool": return t("out.tool", { to: r.to });
      case "connector": return t("out.connector", { to: r.to, n: Number(d.items ?? 0) });
      case "release": return t("out.release", { to: r.to });
      case "review": return t("out.review", { to: r.to, calls: Number(d.calls ?? 0), tokens: Number(d.tokens ?? 0) });
      case "program": return t("out.program", { to: r.to });
      case "gap": return t(Number(d.entries ?? 0) === 1 ? "out.gap.one" : "out.gap", { n: Number(d.entries ?? 0) });
      default: return t("out.held", { where: doorName(r.to), secrets: Number(d.secrets ?? 0), orders: Number(d.instructions ?? 0) });
    }
  };
  return (
    <section class="card stack" id="outbound" aria-labelledby="h-outbound">
      <h2 id="h-outbound">{t("out.title")}</h2>
      <Show when={list()} fallback={<p class="muted" role="status">{list.loading ? t("inbox.loading") : t("privacy.unavailable")}</p>}>
        {(o) => (
          <>
            <ul class="plain stack" aria-live="polite">
              <For each={o().rows} fallback={<li class="ok-text">✓ {t("out.none")}</li>}>
                {(r) => <li class={r.what === "held" ? "out-row held" : "out-row"}><time class="muted">{whenToday(r.at, locale())}</time> <span class={r.what === "held" ? "chip ok" : "chip"}>{t(`out.kind.${r.what}`)}</span> {line(r)}</li>}
              </For>
            </ul>
            <p class="muted">{t("out.note")}</p>
          </>
        )}
      </Show>
    </section>
  );
}

/** One door and the one word for its state: green when nothing goes through it, amber when something can go after a yes,
 *  red when something goes without one. A row links to the card that says more. */
function Glance(props: { f: PrivacyFacts }) {
  const f = () => props.f;
  const modelState = () => (f().model.simulated ? ["ok", "glance.model.test"] : f().model.stays_here ? ["ok", "glance.model.here"] : f().model.class === "verified_tee" ? ["warn", "glance.model.tee"] : ["bad", "glance.model.hosted"]);
  const rows = (): [string, string, string, string][] => [
    ["model", "privacy.model.title", ...(modelState() as [string, string])],
    ["web", "privacy.web.title", ...(f().search.on ? ["warn", "glance.asks"] : ["ok", "glance.off"]) as [string, string]],
    ["telegram", "privacy.tg.title", ...(f().telegram.on ? ["warn", "glance.counts"] : ["ok", "glance.notPaired"]) as [string, string]],
    ["github", "privacy.gh.title", ...(f().github?.connected ? ["warn", "glance.readsWhenAsked"] : ["ok", "glance.notSignedIn"]) as [string, string]],
    ["coding-agent", "privacy.ca.title", ...(f().coding_agent?.installed ? ["warn", "glance.asks"] : ["ok", "glance.none"]) as [string, string]],
    ["extension", "privacy.ext.title", ...(f().extension.on ? ["warn", "glance.paired"] : ["ok", "glance.notPaired"]) as [string, string]],
    ["agents", "privacy.agents.title", ...(f().agents.tools_open.length > 0 ? ["warn", "glance.toolsOpen"] : ["ok", "glance.readOnly"]) as [string, string]],
    ["reviews", "privacy.reviews.title", "ok", "glance.youApprove"],
    ["updates", "privacy.updates.title", "ok", "glance.noTelemetry"],
  ];
  return (
    <section class="card stack" aria-labelledby="h-glance">
      <h2 id="h-glance">{t("glance.title")}</h2>
      <ul class="glance">
        <For each={rows()}>
          {([id, title, tone, word]) => (
            <li>
              <a href={`#pv-${id}`} onClick={(e) => { e.preventDefault(); document.getElementById(`pv-${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" }); }}>{t(title as Key)}</a>
              <span class={`chip ${tone}`}>{tone === "ok" ? "✓ " : ""}{t(word as Key)}</span>
            </li>
          )}
        </For>
      </ul>
    </section>
  );
}

/** The projects whose work may not go to every model, with where each may go: the page that says where data goes says
 *  this too. Nothing is shown when every project may go to the model chosen in Settings. */
function KeptProjects() {
  const [list] = createResource(() => api.projects().catch(() => null));
  const kept = () => (list()?.projects ?? []).filter((p) => p.floor === "here" || p.floor === "confidential");
  return (
    <Show when={kept().length > 0}>
      <section class="card stack" id="pv-projects" aria-labelledby="h-pv-projects">
        <h2 id="h-pv-projects">{t("privacy.projects.title")}</h2>
        <p>{t("privacy.projects.lead")}</p>
        <ul class="plain">
          <For each={kept()}>{(p) => <li><strong>{p.name}</strong> <span class="chip ok">{t(`proj.floor.${p.floor}` as Key)}</span></li>}</For>
        </ul>
        <p><a class="line-link" href="#/projects">{t("privacy.projects.change")}</a></p>
      </section>
    </Show>
  );
}

/** The screen a held-back entry came from, in the reader's language; a name this page does not know is shown as written. */
const DOORS: Record<string, Key> = { chat: "nav.chat", "try to break it": "break.title", documents: "nav.docs", "meeting notes": "nav.meeting" };
function doorName(door: string): string {
  return DOORS[door] ? t(DOORS[door]) : door;
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
            <Glance f={f()} />
            <KeptProjects />
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
            <Card id="github" title="privacy.gh.title">
              <p>{t(f().github?.connected ? "privacy.gh.on" : "privacy.gh.off")}</p>
            </Card>
            <Card id="coding-agent" title="privacy.ca.title">
              <p>{f().coding_agent?.installed ? t("privacy.ca.on", { tool: f().coding_agent?.installed ?? "" }) : t("privacy.ca.off")}</p>
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
