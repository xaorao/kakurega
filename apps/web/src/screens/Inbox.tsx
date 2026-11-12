import { LoadState } from "../components/LoadState";
import { createResource, createSignal, For, onCleanup, Show } from "solid-js";
import { api } from "../api";
import { t, type Key } from "../i18n";
import { notifyState, turnNotifyOn, notifier } from "../notifier";

export function Inbox() {
  const [inbox, { refetch }] = createResource(() => api.inbox().catch(() => null));
  const timer = setInterval(() => void refetch(), 20000);
  const changed = () => void refetch();
  window.addEventListener("kk-changed", changed);
  onCleanup(() => {
    clearInterval(timer);
    window.removeEventListener("kk-changed", changed);
  });
  const [notify, setNotify] = createSignal(notifyState());
  const flip = async (on: boolean) => {
    if (on) await turnNotifyOn();
    else notifier.setEnabled(false);
    setNotify(notifyState());
  };
  return (
    <section class="stack wide" aria-labelledby="h-inbox">
      <h1 id="h-inbox">{t("inbox.title")}</h1>
      <p class="lead">{t("inbox.lead")}</p>
      <div class="card stack">
        <Show when={notify() !== "unsupported"} fallback={<p class="muted">{t("inbox.notify.unsupported")}</p>}>
          <label class="row-check">
            <input type="checkbox" checked={notify() === "on"} onChange={(e) => void flip(e.currentTarget.checked)} aria-describedby="notify-help" />
            <span>{t("inbox.notify.label")}</span>
          </label>
          <p id="notify-help" class="muted">{notify() === "denied" ? t("inbox.notify.denied") : t("inbox.notify.help")}</p>
        </Show>
      </div>
      <Show when={inbox()} fallback={<LoadState loading={inbox.loading} retry={() => void refetch()} />}>
        {(r) => (
          <Show when={r().items.length > 0} fallback={<p class="card" role="status">{t("inbox.empty")}</p>}>
            <ul class="plain stack">
              <For each={r().items}>
                {(i) => (
                  <li class="card stack">
                    <Show when={i.team} fallback={<p><span class="chip">{t(`inbox.kind.${i.kind}` as Key)}</span> <strong>{i.recipient}</strong> · <code>{i.job}</code></p>}>
                      <p><span class="chip">{t(`inbox.kind.${i.kind}` as Key)}</span> <strong>{i.title}</strong></p>
                    </Show>
                    {/* said by the page in its own language; an expert's question is the expert's words and is shown as it came */}
                    <p>{i.kind === "team_question" ? i.what : t(`inbox.what.${i.kind}` as Key, { n: i.count ?? 0 })}</p>
                    <div class="actions"><a class="primary" href={i.team ? `#/team/${i.team}` : `#/job/${i.job}`}>{t("inbox.open")}</a></div>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        )}
      </Show>
    </section>
  );
}
