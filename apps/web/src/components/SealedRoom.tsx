import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { api, type Job } from "../api";
import { t, type Key } from "../i18n";
import { sizeText as size } from "../attach";

type Kind = "plain" | "withheld" | "planted" | "cited";

const LABEL: Record<Kind, Key> = { plain: "room.kind.plain", withheld: "room.kind.withheld", planted: "room.kind.planted", cited: "room.kind.cited" };

/**
 * The whole review in one picture: every file of the project as a dot inside the sealed room, the gate, and the exact
 * files that may leave. The dots are a drawing of facts shown in full on this tab and the Source tab, so the drawing has
 * one spoken summary and is otherwise skipped by a screen reader.
 */
export function SealedRoom(props: { job: Job }) {
  const [files] = createResource(() => props.job.id, (id) => api.files(id).then((r) => r.files).catch(() => []));
  const [findings] = createResource(() => props.job.id, (id) => api.findings(id).then((r) => r.findings).catch(() => []));
  const [release] = createResource(() => props.job.id, (id) => api.release(id).catch(() => null));
  const [hover, setHover] = createSignal<number>(-1);
  const planted = createMemo(() => new Set(props.job.summary?.injection_suspects ?? []));
  const cited = createMemo(() => {
    const out = new Set<string>();
    for (const f of findings() ?? []) if (f.status === "reproduced") out.add(f.file);
    return out;
  });
  const dots = createMemo(() =>
    (files() ?? []).map((f) => {
      const kind: Kind = f.withheld ? "withheld" : planted().has(f.path) ? "planted" : cited().has(f.path) ? "cited" : "plain";
      return { path: f.path, kind };
    }),
  );
  const count = (k: Kind) => dots().filter((d) => d.kind === k).length;
  const leaving = () => (release()?.available ? (release()?.files ?? []) : []);
  const total = () => leaving().reduce((a, f) => a + f.bytes, 0);
  const at = () => dots()[hover()];
  return (
    <Show when={(files() ?? []).length > 0}>
      <section class="room" aria-label={t("room.label")}>
        <div class="room-in">
          <div class="room-head"><h2>{t("room.inside")}</h2><span>{t(dots().length === 1 ? "room.files.one" : "room.files", { n: dots().length })}</span></div>
          <div class={dots().length <= 48 ? "room-dots few" : "room-dots"} role="img" aria-label={t("room.summary", { n: dots().length, withheld: count("withheld"), planted: count("planted"), cited: count("cited") })} onMouseLeave={() => setHover(-1)}>
            <For each={dots()}>{(d, i) => <span class={`room-dot ${d.kind}`} onMouseEnter={() => setHover(i())} />}</For>
          </div>
          <p class="room-hover" aria-hidden="true">
            <span>{at()?.path ?? t("room.hover")}</span>
            <span class={at()?.kind ?? ""}>{at() ? t(LABEL[at()!.kind]) : t("room.counts", { withheld: count("withheld"), planted: count("planted") })}</span>
          </p>
        </div>
        <div class="room-gate" aria-hidden="true"><span>{t("room.gate")}</span></div>
        <div class="room-out">
          <div class="room-head"><h2>{t("room.leaving")}</h2><Show when={leaving().length > 0}><span>{t(leaving().length === 1 ? "room.files.one" : "room.files", { n: leaving().length })} · {size(total())}</span></Show></div>
          <Show when={leaving().length > 0} fallback={<p class="room-none">{t(release() && !release()!.available ? "room.nothing" : "room.notYet")}</p>}>
            <ul class="room-list"><For each={leaving()}>{(f) => <li><span>{f.name}</span><span>{size(f.bytes)}</span></li>}</For></ul>
            <p class="room-note">{t(props.job.delivered ? "room.delivered" : "room.held", { recipient: props.job.recipient })} {t(count("withheld") === 1 ? "room.rest.one" : "room.rest", { n: count("withheld") })}</p>
          </Show>
        </div>
      </section>
    </Show>
  );
}
