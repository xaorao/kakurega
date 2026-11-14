import { Lead } from "../components/Lead";
import { Glyph } from "../components/Glyph";
import { LoadState } from "../components/LoadState";
import { ChosenFolder } from "../components/ChosenFolder";
import { SampleCard } from "../components/SampleCard";
import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api, ApiError, type ChangeView, type Config, type Readiness, stateOf } from "../api";
import { rich, t } from "../i18n";
import { askFromStart } from "../composer";

/** Where the reviewing model runs, said as it is: only a route that stays here is called "on this computer". */
function modelLine(route: Config["route"]) {
  const model = route.model ?? route.id;
  if (route.simulated) return rich("new.model", { model, where: t("new.model.scripted") });
  if (route.stays_here === false) {
    const host = route.host ?? "";
    return rich("new.model.remote", { model, where: t(route.class === "verified_tee" ? "new.model.tee" : "new.model.elsewhere", { host }) });
  }
  return rich("new.model", { model, where: t("new.model.local") });
}

/** The small drawing in front of a way to begin; decoration only. */
function WayIcon(props: { d: string }) {
  return (
    <span class="way-icon" aria-hidden="true">
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"><path d={props.d} /></svg>
    </span>
  );
}

export function NewReview(props: { config: Config; onStarted: (id: string) => void }) {
  const [dir, setDir] = createSignal<string | undefined>(undefined);
  const [listing, { refetch: refetchListing }] = createResource(() => dir() ?? "", (d) => api.fs(d || undefined).catch(() => null));
  const foldersChanged = (e: Event) => {
    const removed = (e as CustomEvent<{ removed?: string }>).detail?.removed;
    const d = dir();
    if (removed && d && (d === removed || d.startsWith(removed + "/"))) setDir(undefined);
    else void refetchListing();
  };
  window.addEventListener("kk:folders-changed", foldersChanged);
  onCleanup(() => window.removeEventListener("kk:folders-changed", foldersChanged));
  const [recipient, setRecipient] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [what, setWhat] = createSignal<"whole" | "change">("whole");
  const [base, setBase] = createSignal("main");
  const [change, setChange] = createSignal<ChangeView | null>(null);
  const [changeError, setChangeError] = createSignal("");
  const [changeBusy, setChangeBusy] = createSignal(false);
  const showChange = async () => {
    setChangeBusy(true);
    setChangeError("");
    setChange(null);
    const [folder, from] = [chosen(), base().trim()];
    try {
      const c = await api.changePreview(folder, from);
      // an answer for a folder or a base that is no longer the one on screen is not shown
      if (folder === chosen() && from === base().trim()) setChange(c);
    } catch (e) {
      setChangeError(e instanceof ApiError ? e.message : t("new.base.error"));
    } finally {
      setChangeBusy(false);
    }
  };
  const ranges = (l: [number, number][]) => l.slice(0, 8).map(([a, b]) => (a === b ? `${a}` : `${a}-${b}`)).join(", ") + (l.length > 8 ? ", …" : "");
  const chosen = () => listing()?.path ?? "";
  /** The folder shown is the owner's whole home folder: a review of it is not offered, a project inside it is. */
  const wholeHome = () => !!props.config.home && chosen().replace(/\/+$/, "") === props.config.home.replace(/\/+$/, "");
  const [ready, { refetch }] = createResource<Readiness | null>(() => api.readiness().catch(() => null));
  const blocked = () => ready()?.ready === false;
  const [repairError, setRepairError] = createSignal("");
  const fixNow = async (id: string) => {
    setRepairError("");
    try {
      await api.repair(id);
      await refetch();
    } catch (e) {
      setRepairError(e instanceof ApiError ? e.message : t("setup.repairFailed"));
    }
  };
  const [every, setEvery] = createSignal("24");
  const [sched, { refetch: refetchSched }] = createResource(() => api.schedules().catch(() => null));
  const [schedError, setSchedError] = createSignal("");
  const addSched = async () => {
    setSchedError("");
    try {
      await api.addSchedule(chosen(), recipient().trim(), Number(every()));
      await refetchSched();
    } catch (e) {
      setSchedError(e instanceof ApiError ? e.message : t("sched.error"));
    }
  };
  const removeSched = async (id: string) => {
    try {
      await api.removeSchedule(id);
      await refetchSched();
    } catch {
      /* the list still shows it, so nothing is hidden */
    }
  };
  const poll = setInterval(() => {
    if (ready()?.checks.some((c) => c.repair?.state === "running")) void refetch();
  }, 1500);
  onCleanup(() => clearInterval(poll));

  const [ask, setAsk] = createSignal("");
  // the owner's own saved prompts, offered where work begins (read from this computer; none are sent anywhere by being shown)
  const [mine] = createResource(() => api.prompts().then((r) => r.prompts.slice(0, 3)).catch(() => []));
  const askNow = (e: Event) => {
    e.preventDefault();
    if (!ask().trim()) return;
    askFromStart(ask().trim());
    location.hash = "#/chat";
  };
  const toReview = (kind: "whole" | "change") => {
    setWhat(kind);
    document.getElementById("h-review")?.scrollIntoView({ behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "start" });
  };
  const hour = new Date().getHours();
  const greeting = () => t(hour < 5 ? "start.night" : hour < 12 ? "start.morning" : hour < 18 ? "start.afternoon" : "start.evening");

  const start = async () => {
    setBusy(true);
    setError("");
    try {
      const r = await api.startJob(chosen(), recipient().trim(), what() === "change" ? base().trim() : undefined);
      props.onStarted(r.id);
    } catch (e) {
      // the service is already running as many reviews as it allows: a normal state, said in the page's language
      setError(e instanceof ApiError ? (e.code === "BUSY" ? t("new.busy") : e.message) : t("new.startError"));
    } finally {
      setBusy(false);
    }
  };

  // what must work before a review can run: drawn above the form when something needs fixing, below it when all is well
  const readiness = () => (
    <>
      <Show when={ready()}>
        {(r) => (
          <div class="card stack" role="region" aria-label={t("setup.region")}>
            <h2>{r().ready ? t("setup.ready") : t("setup.finish")}</h2>
            {/* when every check passed the list is folded away: it is there to read, not in the way */}
            <details class="checks-fold" open={!r().checks.every((c) => c.ok)}>
            <summary>{r().checks.every((c) => c.ok) ? <span class="ok-text">✓ {t("setup.allOk", { n: r().checks.length })}</span> : t("setup.someNot")}</summary>
            <ul class="checks">
              <For each={r().checks}>
                {(c) => (
                  <li>
                    <span class={c.ok ? "ok-text" : c.required === false ? "muted" : "bad"}>{c.ok ? t("setup.ok") : c.required === false ? t("setup.optional") : t("setup.attention")}</span> <strong>{c.label}</strong>: {c.detail}
                    <Show when={c.fix}><div class="fix"><code>{c.fix}</code></div></Show>
                    <Show when={c.repairable && c.repair?.state !== "running"}>
                      <div class="actions"><button type="button" onClick={() => fixNow(c.id)}>{t("setup.fixNow")}</button> <span class="muted">{t("setup.fixNote")}</span></div>
                    </Show>
                    <Show when={c.repair}>{(rp) => <p role="status" class={rp().state === "failed" ? "bad" : "muted"}>{rp().state === "running" ? t("setup.fixRunning", { d: rp().detail }) : rp().state === "failed" ? t("setup.fixFailed", { d: rp().detail }) : t("setup.fixDone")}</p>}</Show>
                  </li>
                )}
              </For>
            </ul>
            </details>
            <Show when={repairError()}><p role="alert" class="bad">{repairError()}</p></Show>
            <Show when={!r().ready}>
              <div class="actions"><button onClick={() => refetch()}>{t("setup.recheck")}</button></div>
            </Show>
          </div>
        )}
      </Show>
    </>
  );

  const [recent] = createResource(() => api.jobs().then((r) => [...r.jobs].sort((a, b) => b.created - a.created).slice(0, 3)).catch(() => []));

  return (
    <section class="stack wide" aria-labelledby="h-new">
      <div class="start">
        <span class="brand-mark hero" aria-hidden="true"><Glyph /></span>
        <h1 id="h-new" class="start-hello">{greeting()}</h1>
        <p class="start-sub">{t("start.sub")}</p>
        <ul class="plain start-ways" aria-label={t("start.ways")}>
          <li><button type="button" class="way" onClick={() => toReview("whole")}><WayIcon d="M2 4.5a1 1 0 0 1 1-1h3.5l1.5 1.5h5a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1zM6 9l1.5 1.5L10.5 7.5" /><span class="way-text"><strong>{t("start.way.review")}</strong><span class="muted">{t("start.way.review.sub")}</span></span></button></li>
          <li><button type="button" class="way" onClick={() => toReview("change")}><WayIcon d="M4.5 3v7M4.5 10a1.800 1.800 0 1 0 0 3.600 1.800 1.800 0 0 0 0-3.600zM11.500 2.400a1.800 1.800 0 1 0 0 3.600 1.800 1.800 0 0 0 0-3.600zM11.500 6v1.500a2 2 0 0 1-2 2h-3" /><span class="way-text"><strong>{t("start.way.change")}</strong><span class="muted">{t("start.way.change.sub")}</span></span></button></li>
          <li><a class="way" href="#/docs"><WayIcon d="M4 2h5.5L12 4.5V14H4zM9.5 2v2.500H12M6 8h4M6 10.500h4" /><span class="way-text"><strong>{t("start.way.docs")}</strong><span class="muted">{t("start.way.docs.sub")}</span></span></a></li>
          <li><a class="way" href="#/meeting"><WayIcon d="M3 3.500h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H7l-3 2.500v-2.500H3a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1zM5 7h6M5 9h4" /><span class="way-text"><strong>{t("start.way.meeting")}</strong><span class="muted">{t("start.way.meeting.sub")}</span></span></a></li>
          <li><a class="way" href="#/explore"><WayIcon d="M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zM10.600 10.600 14 14" /><span class="way-text"><strong>{t("nav.explore")}</strong><span class="muted">{t("start.way.explore.sub")}</span></span></a></li>
          <li><a class="way" href="#/team"><WayIcon d="M6 7.500a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM2.500 13c0-2 1.600-3.500 3.500-3.500S9.500 11 9.500 13M11 4a1.800 1.800 0 0 1 0 3.500M11.500 9.700c1.200.400 2 1.600 2 3.300" /><span class="way-text"><strong>{t("nav.team")}</strong><span class="muted">{t("start.way.team.sub")}</span></span></a></li>
        </ul>
        <Show when={(recent() ?? []).length > 0}>
          <p class="start-recent"><span class="muted">{t("start.recent")}</span><For each={recent()}>{(j) => <a href={`#/job/${j.id}`}><span class={`state ${j.state}`} aria-hidden="true" />{j.repo.split("/").pop()} <span class="muted">· {t(`state.${stateOf(j)}` as never)}</span></a>}</For></p>
        </Show>
        <form class="start-ask" onSubmit={askNow}>
          <label class="visually-hidden" for="start-ask">{t("start.askLabel")}</label>
          <textarea id="start-ask" rows={2} placeholder={t("start.placeholder")} value={ask()} onInput={(e) => setAsk(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey && !e.isComposing) askNow(e); }} />
          <div class="start-ask-row">
            <span class="muted">{props.config.route.simulated ? t("start.whereTest") : props.config.route.stays_here === false ? t("start.whereAway", { model: props.config.route.model ?? props.config.route.id }) : t("start.whereHere")}</span>
            <button class="primary" type="submit" disabled={!ask().trim()}>{t("start.ask")}</button>
          </div>
        </form>
        <Show when={(mine() ?? []).length > 0}>
          <div class="start-mine" role="group" aria-label={t("start.mine")}>
            <span class="muted">{t("start.mine")}</span>
            <For each={mine()}>{(p) => <button type="button" class="chip-button" title={p.text.slice(0, 300)} onClick={() => { setAsk(ask().trim() && ask().trim() !== p.text ? `${ask().trimEnd()}\n${p.text}` : p.text); document.getElementById("start-ask")?.focus(); }}>{p.name}</button>}</For>
          </div>
        </Show>
      </div>
      <h2 id="h-review" class="section-title">{t("new.title")}</h2>
      <p class="greeting">{props.config.route.stays_here === false && !props.config.route.simulated ? t(props.config.route.class === "verified_tee" ? "new.greeting.tee" : "new.greeting.elsewhere", { host: props.config.route.host ?? "" }) : t("new.greeting")}</p>
      <Lead text={t("new.lead")} />
      <Show when={ready()?.ready === false}>{readiness()}</Show>
      <div class="card stack">
        <h2>{t("new.step1")}</h2>
        <ChosenFolder path={chosen()} loading={listing.loading} />
        <div class="browser" role="group" aria-label={t("new.folders")}>
          <Show when={listing()} fallback={<LoadState loading={listing.loading} retry={() => void refetchListing()} />}>
            {(l) => (
              <>
                <Show when={l().parent}>
                  {(p) => <button class="row" onClick={() => setDir(p())}>{t("new.up")}</button>}
                </Show>
                <For each={l().dirs} fallback={<p class="muted">{listing.loading ? t("new.loadingFolders") : t("new.noFolders")}</p>}>
                  {(d) => <button class="row" onClick={() => setDir(d.path)}>{d.name}</button>}
                </For>
              </>
            )}
          </Show>
        </div>
      </div>
      <div class="card stack">
        <h2 id="h-what">{t("new.what")}</h2>
        <fieldset class="choices" aria-labelledby="h-what">
          <label class="row-check"><input type="radio" name="new-what" value="whole" checked={what() === "whole"} onChange={() => setWhat("whole")} /><span>{t("new.what.whole")}</span></label>
          <label class="row-check"><input type="radio" name="new-what" value="change" checked={what() === "change"} onChange={() => setWhat("change")} /><span>{t("new.what.change")}</span></label>
        </fieldset>
        <Show when={what() === "change"}>
          <p class="muted">{t("new.what.changeHelp")}</p>
          <label for="base">{t("new.base.label")}</label>
          <div class="actions">
            <input id="base" value={base()} autocapitalize="off" autocomplete="off" spellcheck={false} onInput={(e) => { setBase(e.currentTarget.value); setChange(null); setChangeError(""); }} />
            <button type="button" disabled={changeBusy() || !chosen() || base().trim().length === 0} onClick={showChange}>{t("new.base.show")}</button>
          </div>
          <Show when={changeError()}><p role="alert" class="bad">{changeError()}</p></Show>
          <Show when={change()}>
            {(c) => (
              <div class="stack" role="status">
                <p><strong>{c().files.length === 1 ? t("new.base.count.one") : t("new.base.count", { n: c().files.length })}</strong> · <span class="muted">{t("new.base.commits", { head: c().head.slice(0, 12), base: c().base.slice(0, 12) })}</span></p>
                <ul class="plain change-list">
                  <For each={c().files.slice(0, 200)}>
                    {(f) => <li><code>{f.path}</code> <span class="muted">{t(`new.base.kind.${f.kind}`)}{f.lines.length > 0 ? ` · ${t("new.base.lines", { l: ranges(f.lines) })}` : ""}</span></li>}
                  </For>
                </ul>
                <Show when={(c().edited_since_commit ?? 0) > 0}><p class="warn">{t("new.base.edited", { n: c().edited_since_commit ?? 0 })}</p></Show>
                <Show when={(c().left_out ?? []).length > 0}><p class="muted">{t("new.base.leftOut", { n: (c().left_out ?? []).length })}</p></Show>
              </div>
            )}
          </Show>
        </Show>
      </div>
      <div class="card stack">
        <h2>{t("new.step2")}</h2>
        <label for="recipient">{t("new.recipient.label")}</label>
        <input id="recipient" placeholder={t("new.recipient.placeholder")} value={recipient()} onInput={(e) => setRecipient(e.currentTarget.value)} />
        <p class="muted">{t("new.recipient.help")}</p>
      </div>
      <div class="actions">
        <button class="primary" disabled={busy() || blocked() || !chosen() || wholeHome() || recipient().trim().length === 0 || (what() === "change" && base().trim().length === 0)} onClick={start}>{t("new.start")}</button>
        <Show when={wholeHome()}><span class="muted" role="status">{t("new.wholeHome")}</span></Show>
        <Show when={error()}><span role="alert" class="bad">{error()}</span></Show>
      </div>
      <Show when={ready()?.ready !== false}>{readiness()}</Show>
      <SampleCard onReview={(folder) => { setDir(folder); document.getElementById("h-review")?.scrollIntoView({ block: "start" }); }} />
      <details class="card stack" id="schedules">
        <summary>{t("sched.title")}</summary>
        <p class="muted">{t("sched.lead")}</p>
        <div class="actions">
          <label for="every">{t("sched.every")}</label>
          <select id="every" value={every()} onChange={(e) => setEvery(e.currentTarget.value)}>
            <option value="1">{t("sched.hour")}</option>
            <option value="6">{t("sched.hours", { n: 6 })}</option>
            <option value="24">{t("sched.day")}</option>
            <option value="168">{t("sched.week")}</option>
          </select>
          <button type="button" disabled={!chosen() || recipient().trim().length === 0} onClick={addSched}>{t("sched.add")}</button>
          <Show when={schedError()}><span role="alert" class="bad">{schedError()}</span></Show>
        </div>
        <Show when={!sched() && !sched.loading}><LoadState loading={false} retry={() => void refetchSched()} /></Show>
        <Show when={(sched()?.schedules ?? []).length > 0} fallback={<Show when={sched()}><p class="muted">{t("sched.none")}</p></Show>}>
          <ul class="plain stack">
            <For each={sched()?.schedules}>
              {(x) => (
                <li class="actions">
                  <span><code>{x.repo}</code> → {x.recipient} · {t("sched.cadence", { n: x.every_hours })}</span>
                  <button type="button" onClick={() => removeSched(x.id)}>{t("sched.remove")}</button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </details>
      <p class="muted">{modelLine(props.config.route)}</p>
    </section>
  );
}
