import { createEffect, For, onCleanup, Show } from "solid-js";
import { t, type Key } from "../i18n";
import { keysFor, SHORTCUTS, type Scope } from "../shortcuts";

const isMac = () => typeof navigator !== "undefined" && /mac|iphone|ipad/i.test(navigator.platform || navigator.userAgent);

/** The shortcuts the workspace answers to, generated from the one table that also finds them. */
export function ShortcutHelp(props: { open: boolean; onClose: () => void }) {
  let box: HTMLDivElement | undefined;
  let opener: Element | null = null;
  createEffect(() => {
    if (props.open) {
      opener = document.activeElement;
      queueMicrotask(() => box?.focus());
    } else if (opener instanceof HTMLElement) {
      opener.focus();
      opener = null;
    }
  });
  const onKey = (e: KeyboardEvent) => {
    if (!props.open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    } else if (e.key === "Tab") {
      e.preventDefault(); // the dialog is one stop: focus stays on it until it closes
    }
  };
  window.addEventListener("keydown", onKey, true);
  onCleanup(() => window.removeEventListener("keydown", onKey, true));
  const group = (scope: Scope) => SHORTCUTS.filter((s) => s.scope === scope);
  return (
    <Show when={props.open}>
      <div class="palette-backdrop" onMouseDown={(e) => e.target === e.currentTarget && props.onClose()}>
        <div ref={box} class="palette card kb-help" role="dialog" aria-modal="true" aria-labelledby="kb-title" tabindex="-1">
          <h2 id="kb-title">{t("kb.title")}</h2>
          <For each={["app", "chat"] as Scope[]}>
            {(scope) => (
              <section class="stack" aria-label={t(`kb.scope.${scope}` as Key)}>
                <h3>{t(`kb.scope.${scope}` as Key)}</h3>
                <dl class="kb-list">
                  <For each={group(scope)}>
                    {(s) => (
                      <>
                        <dt><kbd>{keysFor(s, isMac())}</kbd></dt>
                        <dd>{t(s.label as Key)}</dd>
                      </>
                    )}
                  </For>
                </dl>
              </section>
            )}
          </For>
          <div class="actions"><button type="button" onClick={props.onClose}>{t("kb.close")}</button></div>
        </div>
      </div>
    </Show>
  );
}
