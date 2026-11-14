import { Lead } from "../components/Lead";
import { createMemo, createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api, ApiError, streamRoundtable, type ExpertInfo, type RoundtableResolution } from "../api";
import { LoadState } from "../components/LoadState";
import { Markdown } from "../components/Markdown";
import { t } from "../i18n";
import { ExpertCard } from "../components/ExpertCard";

type Seat = { id: string; name: string; state: "waiting" | "asking" | "answered" | "failed"; text: string; why: string; cut: boolean; short: boolean };

const STORE = "kk.roundtable.experts";

function remembered(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(STORE) ?? "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x === "string").slice(0, 5) : [];
  } catch {
    return [];
  }
}

/** One question to several experts, each answering alone, then a lead's resolution in four parts. */
export function Roundtable() {
  // a roster that could not be read is said so with a way to try again, not shown as an empty list
  const [roster, { refetch: refetchRoster }] = createResource(() => api.experts().catch(() => null));
  // the built-in experts, then the owner's own approved methods, which can be asked as experts
  const everyone = createMemo<ExpertInfo[]>(() => [...(roster()?.experts ?? []), ...(roster()?.yours ?? []).map((y) => ({ ...y, group: t("rt.yours") }))]);
  const [picked, setPicked] = createSignal<string[]>(remembered());
  const [question, setQuestion] = createSignal("");
  const [seats, setSeats] = createSignal<Seat[]>([]);
  const [resolution, setResolution] = createSignal<RoundtableResolution | null>(null);
  const [noResolution, setNoResolution] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [stopped, setStopped] = createSignal(false);
  const [error, setError] = createSignal("");
  let stop: AbortController | null = null;
  onCleanup(() => stop?.abort());

  const groups = createMemo(() => {
    const out: { group: string; experts: ExpertInfo[] }[] = [];
    for (const e of everyone()) {
      const g = out.find((x) => x.group === e.group);
      if (g) g.experts.push(e);
      else out.push({ group: e.group, experts: [e] });
    }
    return out;
  });
  const min = () => roster()?.min ?? 2;
  const max = () => roster()?.max ?? 5;
  // a remembered choice that this build no longer offers is dropped, not sent
  const chosen = createMemo(() => picked().filter((id) => everyone().some((e) => e.id === id)));
  const ready = () => chosen().length >= min() && chosen().length <= max() && question().trim().length > 0 && !busy();

  const toggle = (id: string) => {
    const now = chosen().includes(id) ? chosen().filter((x) => x !== id) : chosen().length < max() ? [...chosen(), id] : chosen();
    setPicked(now);
    try {
      localStorage.setItem(STORE, JSON.stringify(now));
    } catch {
      /* the choice is a convenience; the page works without it */
    }
  };

  const seat = (id: string, change: (s: Seat) => Seat) => setSeats((all) => all.map((s) => (s.id === id ? change(s) : s)));

  const ask = async (e: Event) => {
    e.preventDefault();
    if (!ready()) return;
    setError("");
    setStopped(false);
    setResolution(null);
    setNoResolution("");
    const names = new Map(everyone().map((x) => [x.id, x.name]));
    setSeats(chosen().map((id) => ({ id, name: names.get(id) ?? id, state: "waiting", text: "", why: "", cut: false, short: false })));
    setBusy(true);
    stop = new AbortController();
    try {
      await streamRoundtable(
        { message: question().trim(), experts: chosen() },
        (v) => {
          if (typeof v.expert === "string") {
            const id = v.expert;
            if (v.state === "asking") seat(id, (s) => ({ ...s, state: "asking" }));
            else if (typeof v.delta === "string") seat(id, (s) => ({ ...s, text: s.text + v.delta }));
            else if (typeof v.answer === "string") seat(id, (s) => ({ ...s, state: "answered", text: v.answer as string, cut: v.truncated_for_lead === true, short: v.cut_short === true }));
            else if (typeof v.failed === "string") seat(id, (s) => ({ ...s, state: "failed", text: "", why: v.failed as string }));
          } else if ("resolution" in v) {
            if (v.resolution) setResolution(v.resolution as RoundtableResolution);
            else setNoResolution(typeof v.reason === "string" ? v.reason : t("rt.noResolution"));
          } else if (v.stopped) {
            setStopped(true);
          }
        },
        stop.signal,
      );
    } catch (err) {
      if (stop?.signal.aborted) setStopped(true);
      else setError(err instanceof ApiError ? err.message : t("rt.error"));
    } finally {
      // whoever had not answered when it ended is no longer being asked
      setSeats((all) => all.map((s) => (s.state === "asking" || s.state === "waiting" ? { ...s, state: "failed", why: t("rt.notAsked") } : s)));
      setBusy(false);
      stop = null;
    }
  };

  const chip = (s: Seat) => (s.state === "answered" ? "chip ok" : s.state === "failed" ? "chip bad" : "chip");
  const label = (s: Seat) => t(s.state === "answered" ? "rt.state.answered" : s.state === "failed" ? "rt.state.failed" : s.state === "asking" ? "rt.state.asking" : "rt.state.waiting");

  return (
    <section class="stack" aria-labelledby="rt-title">
      <h1 id="rt-title">{t("rt.title")}</h1>
      <Lead text={t("rt.lead")} tail={1} />
      <form class="card stack" onSubmit={ask}>
        <label for="rt-question">{t("rt.question")}</label>
        <textarea id="rt-question" rows="3" maxlength="4000" value={question()} disabled={busy()} onInput={(e) => setQuestion(e.currentTarget.value)} />
        <fieldset class="rt-experts">
          <legend>{t("rt.choose", { min: min(), max: max() })}</legend>
          <Show when={roster()} fallback={<LoadState loading={roster.loading} retry={() => void refetchRoster()} />}>
            <Show when={groups().length === 0}><p role="alert" class="bad">{t("rt.noRoster")}</p></Show>
          </Show>
          <For each={groups()}>
            {(g) => (
              <div class="rt-group">
                <h2>{g.group}</h2>
                <div class="rt-cards">
                  <For each={g.experts}>
                    {(x) => (
                      <ExpertCard id={x.id} name={x.name} summary={x.summary} checked={chosen().includes(x.id)} disabled={busy() || (!chosen().includes(x.id) && chosen().length >= max())} onChange={() => toggle(x.id)} />
                    )}
                  </For>
                </div>
              </div>
            )}
          </For>
          <p class="muted" aria-live="polite">{t("rt.count", { n: chosen().length, min: min(), max: max() })}</p>
        </fieldset>
        <div class="rt-actions">
          <button class="primary" type="submit" disabled={!ready()}>{t("rt.ask")}</button>
          <Show when={busy()}><button type="button" onClick={() => stop?.abort()}>{t("chat.stop")}</button></Show>
          <span class="muted">{t("rt.notKept")}</span>
        </div>
        <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
        <Show when={stopped()}><p role="status" class="muted">{t("rt.stopped")}</p></Show>
      </form>

      <Show when={resolution() || noResolution()}>
        <section class="card stack rt-resolution" aria-labelledby="rt-res">
          <h2 id="rt-res">{t("rt.resolution")}</h2>
          <Show when={resolution()} fallback={<p role="status">{noResolution()}</p>}>
            {(r) => (
              <Show
                when={r().structured}
                fallback={<><p class="muted">{t("rt.unstructured")}</p><Markdown text={r().text} /></>}
              >
                <dl class="rt-parts">
                  <dt>{t("rt.decision")}</dt>
                  <dd class="rt-decision"><Show when={r().decision} fallback={<p class="muted">{t("rt.partEmpty")}</p>}><Markdown text={r().decision ?? ""} /></Show></dd>
                  <dt>{t("rt.agreed")}</dt>
                  <dd><Show when={r().agreed} fallback={<p class="muted">{t("rt.partEmpty")}</p>}><Markdown text={r().agreed ?? ""} /></Show></dd>
                  <dt>{t("rt.disputed")}</dt>
                  <dd><Show when={r().disputed} fallback={<p class="muted">{t("rt.partEmpty")}</p>}><Markdown text={r().disputed ?? ""} /></Show></dd>
                  <dt>{t("rt.open")}</dt>
                  <dd><Show when={r().open_questions} fallback={<p class="muted">{t("rt.partEmpty")}</p>}><Markdown text={r().open_questions ?? ""} /></Show></dd>
                </dl>
              </Show>
            )}
          </Show>
          <p class="muted">{t("rt.resolutionNote")}</p>
        </section>
      </Show>

      <Show when={seats().length > 0}>
        <h2>{t("rt.answers")}</h2>
        <ul class="plain stack">
          <For each={seats()}>
            {(s) => (
              <li class="card stack">
                <p><strong>{s.name}</strong> <span class={chip(s)}>{label(s)}</span></p>
                <Show when={s.text}><Markdown text={s.text} /></Show>
                <Show when={s.state === "failed"}><p class="muted">{s.why}</p></Show>
                <Show when={s.short}><p class="muted">{t("chat.cutShort")}</p></Show>
                <Show when={s.cut}><p class="muted">{t("rt.cut")}</p></Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </section>
  );
}
