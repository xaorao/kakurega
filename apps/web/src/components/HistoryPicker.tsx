import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import { t } from "../i18n";
import { historyMatches } from "../composer";
import { moveSelection } from "../palette-match";

/** Earlier messages from this page, newest first, narrowed by typing. Enter puts one in the message box; Escape leaves the box as it was. */
export function HistoryPicker(props: { entries: readonly string[]; onPick: (text: string) => void; onClose: () => void }) {
  const [q, setQ] = createSignal("");
  const [sel, setSel] = createSignal(0);
  const shown = createMemo(() => historyMatches(props.entries, q()));
  let input: HTMLInputElement | undefined;
  onMount(() => input?.focus());
  const onKey = (e: KeyboardEvent) => {
    if (e.isComposing) return;
    const n = shown().length;
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "PageDown" || e.key === "PageUp") {
      e.preventDefault();
      setSel(moveSelection(sel(), n, e.key === "ArrowDown" ? "down" : e.key === "ArrowUp" ? "up" : e.key === "PageDown" ? "pageDown" : "pageUp"));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const pick = shown()[sel()];
      if (pick !== undefined) props.onPick(pick);
    } else if (e.key === "Tab") {
      e.preventDefault();
    }
  };
  return (
    <div class="card stack history-picker" role="group" aria-label={t("chat.pick.title")}>
      <input
        ref={input}
        type="text"
        role="combobox"
        aria-expanded="true"
        aria-controls="history-list"
        aria-activedescendant={shown()[sel()] !== undefined ? `hist-${sel()}` : undefined}
        aria-label={t("chat.pick.title")}
        placeholder={t("chat.pick.placeholder")}
        value={q()}
        onInput={(e) => {
          setQ(e.currentTarget.value);
          setSel(0);
        }}
        onKeyDown={onKey}
      />
      <ul id="history-list" role="listbox" class="plain">
        <For each={shown()} fallback={<li class="muted" role="status">{t("chat.pick.none")}</li>}>
          {(m, idx) => (
            <li id={`hist-${idx()}`} role="option" aria-selected={idx() === sel()} class={idx() === sel() ? "pal-item sel" : "pal-item"} onMouseMove={() => setSel(idx())} onClick={() => props.onPick(m)}>
              <span class="hist-text">{m}</span>
            </li>
          )}
        </For>
      </ul>
    </div>
  );
}
