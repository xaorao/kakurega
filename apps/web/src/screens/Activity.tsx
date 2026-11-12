import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api, ApiError, type RoutePrice, type WorkItem } from "../api";
import { LoadState } from "../components/LoadState";
import { rich, t, type Key } from "../i18n";

function label(kind: string): string {
  const key = `log.kind.${kind}` as Key;
  const text = t(key);
  return text === key ? kind : text;
}

const DAY = 86_400;
const day = (secs: number) => new Date(secs * 1000).toISOString().slice(0, 10);

/** The prices the owner typed for a million tokens read and written, per model route. This program knows no provider's prices. */
function Prices(props: { routes: string[]; onChange: () => void }) {
  const [list, { refetch }] = createResource(() => api.prices().then((r): RoutePrice[] | null => r.prices).catch(() => null));
  const [route, setRoute] = createSignal("");
  const [inp, setInp] = createSignal("");
  const [out, setOut] = createSignal("");
  const [cur, setCur] = createSignal("USD");
  const [msg, setMsg] = createSignal("");
  const [err, setErr] = createSignal("");
  const run = async (fn: () => Promise<unknown>, said: string) => {
    setErr("");
    setMsg("");
    try {
      await fn();
      setMsg(said);
      await refetch();
      props.onChange();
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : t("use.price.error"));
    }
  };
  const save = (e: Event) => {
    e.preventDefault();
    const n = (v: string) => (v.trim() === "" ? NaN : Number(v));
    void run(() => api.setPrice({ route: route().trim(), input_per_million: n(inp()), output_per_million: n(out()), currency: cur().trim().toUpperCase() }), t("use.price.saved"));
  };
  return (
    <details class="stack" id="prices">
      <summary>{t("use.price.title")}</summary>
      <p class="muted">{t("use.price.lead")}</p>
      <form class="stack" onSubmit={save}>
        <label for="price-route">{t("use.price.route")}</label>
        <input id="price-route" list="price-routes" value={route()} maxlength="80" onInput={(e) => setRoute(e.currentTarget.value)} />
        <datalist id="price-routes"><For each={props.routes}>{(r) => <option value={r} />}</For></datalist>
        <label for="price-in">{t("use.price.in")}</label>
        <input id="price-in" inputmode="decimal" value={inp()} onInput={(e) => setInp(e.currentTarget.value)} />
        <label for="price-out">{t("use.price.out")}</label>
        <input id="price-out" inputmode="decimal" value={out()} onInput={(e) => setOut(e.currentTarget.value)} />
        <label for="price-cur">{t("use.price.currency")}</label>
        <input id="price-cur" value={cur()} maxlength="3" onInput={(e) => setCur(e.currentTarget.value)} />
        <div class="actions"><button class="primary" type="submit" disabled={route().trim() === ""}>{t("use.price.save")}</button></div>
      </form>
      <Show when={msg()}><p role="status" class="ok">{msg()}</p></Show>
      <Show when={err()}><p role="alert" class="bad">{err()}</p></Show>
      <Show when={list()} fallback={<LoadState loading={list.loading} retry={() => void refetch()} />}>
      <ul class="plain stack" aria-label={t("use.price.entered")}>
        <For each={list()} fallback={<li class="muted">{t("use.price.none")}</li>}>
          {(p) => (
            <li class="actions">
              <code>{p.route}</code>
              <span>{t("use.price.line", { in: String(p.input_per_million), out: String(p.output_per_million), cur: p.currency })}</span>
              <button type="button" onClick={() => void run(() => api.removePrice(p.route), t("use.price.removed"))}>{t("use.price.remove")} <span class="sr">{p.route}</span></button>
            </li>
          )}
        </For>
      </ul>
      </Show>
    </details>
  );
}

const money = (amount: number, currency: string) => `${currency} ${amount.toFixed(4)}`;

/** What each model route answered over a range of days: replies, tokens in and out, how fast, and a cost where the owner typed a price. */
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
  const [usage, { refetch: refetchUsage }] = createResource(range, async (r) => {
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
          <>
          <Show when={u().routes.length > 0} fallback={<p>{t("use.none")}</p>}>
            <div class="table-scroll" tabindex="0" role="region" aria-label={t("use.col.route")}>
            <table>
              <thead><tr><th scope="col">{t("use.col.route")}</th><th scope="col">{t("use.col.replies")}</th><th scope="col">{t("use.col.in")}</th><th scope="col">{t("use.col.out")}</th><th scope="col">{t("use.col.speed")}</th><Show when={u().routes.some((r) => r.cost)}><th scope="col">{t("use.col.cost")}</th></Show></tr></thead>
              <tbody>
                <For each={u().routes}>
                  {(r) => (
                    <tr>
                      <td>{r.route}{r.simulated ? ` (${t("chat.test")})` : ""}</td>
                      <td>{r.turns}</td>
                      <td>{r.prompt_tokens}{r.measured_turns < r.turns ? "*" : ""}</td>
                      <td>{r.reply_tokens}</td>
                      <td>{t("use.speed", { ms: r.avg_ms, tps: r.reply_tokens_per_second })}</td>
                      <Show when={u().routes.some((x) => x.cost)}><td>{r.cost ? `${money(r.cost.amount, r.cost.currency)}${r.cost.tokens_estimated ? "*" : ""}` : "–"}</td></Show>
                    </tr>
                  )}
                </For>
                <tr><th scope="row">{t("use.total")}</th><td>{u().total.turns}</td><td>{u().total.prompt_tokens}</td><td>{u().total.reply_tokens}</td><td></td><Show when={u().routes.some((r) => r.cost)}><td>{u().total.cost.map((c) => money(c.amount, c.currency)).join(", ")}</td></Show></tr>
              </tbody>
            </table>
            </div>
            <p class="muted">{t("use.note")}</p>
          </Show>
          <Prices routes={u().routes.filter((r) => !r.simulated).map((r) => r.route)} onChange={() => void refetchUsage()} />
          </>
        )}
      </Show>
    </section>
  );
}

/** Everything that is working now, with a warning on what has shown no progress for a while. */
function WorkingNow() {
  const [work, { refetch }] = createResource(() => api.work().catch(() => null));
  const [err, setErr] = createSignal("");
  const timer = setInterval(() => void refetch(), 5000);
  onCleanup(() => clearInterval(timer));
  const link = (i: WorkItem) => (i.kind === "review" ? `#/job/${i.id}` : i.kind === "team" ? `#/team/${i.id}` : "#/code");
  const minutes = (secs: number) => Math.max(1, Math.round(secs / 60));
  const stop = async (i: WorkItem) => {
    setErr("");
    try {
      if (i.kind === "team") await api.teamStop(i.id);
      else await api.stopWork(i.kind, i.id);
      await refetch();
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : t("work.stopError"));
    }
  };
  return (
    <section class="card stack" id="working" aria-labelledby="h-work">
      <h2 id="h-work">{t("work.title")}</h2>
      <Show when={work()} fallback={<LoadState loading={work.loading} retry={() => void refetch()} />}>
        {(w) => (
          <Show when={w().items.length > 0} fallback={<p class="muted" role="status">{t("work.none")}</p>}>
            <Show when={w().quiet > 0}><p role="status" class="chip warn">{t("work.quietCount", { n: w().quiet })}</p></Show>
            <ul class="plain stack work-list">
              <For each={w().items}>
                {(i) => (
                  <li class="work-item" data-kind={i.kind} data-quiet={i.quiet ? "yes" : "no"}>
                    <span class="work-what"><span class="chip">{t(`work.kind.${i.kind}`)}</span> <strong>{i.title}</strong></span>
                    <span class={i.quiet ? "chip warn" : "muted"}>{i.quiet ? t("work.quiet", { n: minutes(i.quiet_for) }) : t("work.alive", { n: minutes(i.quiet_for) })}</span>
                    <span class="actions">
                      <a class="button" href={link(i)}>{t("inbox.open")}</a>
                      <Show when={i.can_stop} fallback={<span class="muted">{t("work.noStop")}</span>}><button type="button" onClick={() => void stop(i)}>{t("work.stop")}</button></Show>
                    </span>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        )}
      </Show>
      <Show when={err()}><p role="alert" class="bad">{err()}</p></Show>
    </section>
  );
}

export function Activity() {
  const [log, { refetch }] = createResource(() => api.log().catch(() => null));
  return (
    <section class="stack wide" aria-labelledby="h-act">
      <h1 id="h-act">{t("act.title")}</h1>
      <p class="lead">{t("act.lead")}</p>
      <WorkingNow />
      <UsageCard />
      <Show when={log()} fallback={<LoadState loading={log.loading} retry={() => void refetch()} />}>
        {(l) => (
          <>
            <div class="card stack">
              <p>{t("act.count", { count: l().count })}</p>
              <p class="muted">{t("act.head")}: <code>{l().head ?? "-"}</code></p>
              <p class="muted">{t("act.key")}: <code>{l().public_key}</code></p>
              <p><a class="button" href="/api/log/export" download="kakurega-event-log.json">{t("act.download")}</a></p>
              <p class="muted">{rich("act.verify", { key: l().public_key })}</p>
            </div>
            <div class="card table-scroll" tabindex="0" role="region" aria-label={t("act.col.event")}>
              <table>
                <thead><tr><th scope="col">{t("act.col.when")}</th><th scope="col">{t("act.col.event")}</th><th scope="col">{t("act.col.subject")}</th></tr></thead>
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
