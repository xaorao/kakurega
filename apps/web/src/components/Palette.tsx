import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js";
import { t } from "../i18n";

export interface PaletteItem {
  id: string;
  label: string;
  group: string;
  run: () => void;
}

/** Items whose label or group contains every word typed, in the order given. */
export function matchItems(items: PaletteItem[], query: string): PaletteItem[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return items.filter((i) => {
    const hay = `${i.label} ${i.group}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export function Palette(props: { open: boolean; items: PaletteItem[]; onClose: () => void }) {
  const [q, setQ] = createSignal("");
  const [sel, setSel] = createSignal(0);
  const shown = createMemo(() => matchItems(props.items, q()));
  let input: HTMLInputElement | undefined;
  let opener: Element | null = null;
  createEffect(() => {
    if (props.open) {
      opener = document.activeElement;
      setQ("");
      setSel(0);
      queueMicrotask(() => input?.focus());
    } else if (opener instanceof HTMLElement) {
      opener.focus();
      opener = null;
    }
  });
  const choose = (i: PaletteItem | undefined) => {
    if (!i) return;
    props.onClose();
    i.run();
  };
  const onKey = (e: KeyboardEvent) => {
    if (!props.open) return;
    const n = shown().length;
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel(n ? (sel() + 1) % n : 0);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel(n ? (sel() + n - 1) % n : 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(shown()[sel()]);
    } else if (e.key === "Tab") {
      e.preventDefault(); // the palette is one control: focus stays in the search box
    }
  };
  window.addEventListener("keydown", onKey, true);
  onCleanup(() => window.removeEventListener("keydown", onKey, true));
  return (
    <Show when={props.open}>
      <div class="palette-backdrop" onMouseDown={(e) => e.target === e.currentTarget && props.onClose()}>
        <div class="palette card" role="dialog" aria-modal="true" aria-label={t("palette.title")}>
          <input
            ref={input}
            type="text"
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            aria-activedescendant={shown()[sel()] ? `pal-${shown()[sel()]?.id}` : undefined}
            aria-label={t("palette.title")}
            placeholder={t("palette.placeholder")}
            value={q()}
            onInput={(e) => {
              setQ(e.currentTarget.value);
              setSel(0);
            }}
          />
          <ul id="palette-list" role="listbox" class="plain">
            <For each={shown()} fallback={<li class="muted" role="status">{t("palette.none")}</li>}>
              {(i, idx) => (
                <li id={`pal-${i.id}`} role="option" aria-selected={idx() === sel()} class={idx() === sel() ? "pal-item sel" : "pal-item"} onMouseMove={() => setSel(idx())} onClick={() => choose(i)}>
                  <span>{i.label}</span>
                  <span class="muted">{i.group}</span>
                </li>
              )}
            </For>
          </ul>
        </div>
      </div>
    </Show>
  );
}
