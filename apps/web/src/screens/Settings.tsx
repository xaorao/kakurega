import { createMemo, createSignal, For, Show } from "solid-js";
import { LOCALES, locale, setLocale, t, type Key, type Locale } from "../i18n";
import { notifier, notifyState, turnNotifyOn } from "../notifier";
import { SECTIONS, SETTINGS, searchSettings, type Setting } from "../settings-catalog";
import { chooseTheme, theme, type Theme } from "../theme";

function Control(props: { s: Setting }) {
  const [notify, setNotify] = createSignal(notifyState());
  const flip = async (on: boolean) => {
    if (on) await turnNotifyOn();
    else notifier.setEnabled(false);
    setNotify(notifyState());
  };
  return (
    <>
      {props.s.kind === "theme" && (
        <select id={`set-${props.s.id}`} aria-labelledby={`lab-${props.s.id}`} value={theme()} onChange={(e) => chooseTheme(e.currentTarget.value as Theme)}>
          <option value="system">{t("theme.system")}</option>
          <option value="light">{t("theme.light")}</option>
          <option value="dark">{t("theme.dark")}</option>
        </select>
      )}
      {props.s.kind === "language" && (
        <select id={`set-${props.s.id}`} aria-labelledby={`lab-${props.s.id}`} value={locale()} onChange={(e) => setLocale(e.currentTarget.value as Locale)}>
          <For each={LOCALES}>{([code, name]) => <option value={code}>{name}</option>}</For>
        </select>
      )}
      {props.s.kind === "notify" && (
        <Show when={notify() !== "unsupported"} fallback={<p class="muted">{t("inbox.notify.unsupported")}</p>}>
          <label class="row">
            <input id={`set-${props.s.id}`} type="checkbox" checked={notify() === "on"} onChange={(e) => void flip(e.currentTarget.checked)} />
            <span>{notify() === "denied" ? t("inbox.notify.denied") : t("inbox.notify.label")}</span>
          </label>
        </Show>
      )}
      {props.s.kind === "link" && <a class="button" href={props.s.target}>{t("set.open")}</a>}
      {props.s.kind === "shortcuts" && <button type="button" onClick={() => window.dispatchEvent(new Event("kk-show-shortcuts"))}>{t("set.keys.open")}</button>}
    </>
  );
}

/** Every setting in one place, searchable by name, by what it does, or by a word people use for it. */
export function Settings() {
  const [q, setQ] = createSignal("");
  const found = createMemo(() => searchSettings(SETTINGS, q(), (k) => t(k as Key)));
  return (
    <section class="stack wide" aria-labelledby="h-set">
      <h1 id="h-set">{t("set.title")}</h1>
      <p class="lead">{t("set.lead")}</p>
      <label class="sr" for="set-search">{t("set.search")}</label>
      <input id="set-search" type="search" maxlength="100" placeholder={t("set.search")} value={q()} onInput={(e) => setQ(e.currentTarget.value)} />
      <Show when={q().trim() !== ""}><p class="muted" role="status">{found().length === 0 ? t("set.none") : t("set.found", { n: found().length })}</p></Show>
      <For each={SECTIONS}>
        {(sec) => (
          <Show when={found().some((s) => s.section === sec)}>
            <section class="card stack" aria-labelledby={`sec-${sec}`}>
              <h2 id={`sec-${sec}`}>{t(`set.section.${sec}` as Key)}</h2>
              <ul class="plain stack">
                <For each={found().filter((s) => s.section === sec)}>
                  {(s) => (
                    <li class="stack" id={`setting-${s.id}`}>
                      <strong id={`lab-${s.id}`}>{t(s.label as Key)}</strong>
                      <p class="muted">{t(s.help as Key)}</p>
                      <Control s={s} />
                    </li>
                  )}
                </For>
              </ul>
            </section>
          </Show>
        )}
      </For>
    </section>
  );
}
