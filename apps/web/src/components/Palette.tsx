import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js";
import { t } from "../i18n";
import { matchItems, moveSection, moveSelection, sectionHeads } from "../palette-match";

export interface PaletteItem {
  id: string;
  /** The name, in ink. */
  label: string;
  /** The heading the row sits under while browsing. */
  group: string;
  /** One word for what the row is (a screen, a review, a chat), muted at the end of the row. */
  kind: string;
  /** Muted words after the name: a review's state and recipient, a chat's length. */
  detail?: string;
  /** Words that find the row without being shown (a review's id). */
  keywords?: string;
  run: () => void;
}

/** The one place to find a screen, a review or a conversation and to run a command. Rows the service finds by what was said
 *  in a conversation (`search`) come after the rows found by name, so what is under the cursor does not move as they arrive. */
export function Palette(props: { open: boolean; items: PaletteItem[]; onClose: () => void; search?: (q: string) => Promise<PaletteItem[]> }) {
  const [q, setQ] = createSignal("");
  const [sel, setSel] = createSignal(0);
  const [found, setFound] = createSignal<{ q: string; items: PaletteItem[] }>({ q: "", items: [] });
  const named = createMemo(() => matchItems(props.items, q()));
  const shown = createMemo(() => {
    const first = named();
    const extra = found().q === q().trim() ? found().items : [];
    if (extra.length === 0) return first;
    const have = new Set(first.map((i) => i.id));
    return [...first, ...extra.filter((i) => !have.has(i.id))];
  });
  const heads = createMemo(() => new Set(sectionHeads(shown(), q())));
  let input: HTMLInputElement | undefined;
  let opener: Element | null = null;
  // the newest question wins: an answer to an older one is dropped, and nothing is asked once the palette is closed
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const ask = (value: string) => {
    clearTimeout(timer);
    const mine = ++seq;
    const query = value.trim();
    if (!props.search || query.length < 2) {
      setFound({ q: query, items: [] });
      return;
    }
    timer = setTimeout(async () => {
      try {
        const items = await props.search!(query);
        if (mine === seq && props.open) setFound({ q: query, items });
      } catch {
        if (mine === seq) setFound({ q: query, items: [] });
      }
    }, 160);
  };
  onCleanup(() => clearTimeout(timer));
  createEffect(() => {
    // keep the highlighted row in view when arrow keys move past the visible part of the list
    const row = props.open ? shown()[sel()] : undefined;
    if (row) document.getElementById(`pal-${row.id}`)?.scrollIntoView?.({ block: "nearest" });
  });
  createEffect(() => {
    if (props.open) {
      opener = document.activeElement;
      setQ("");
      setSel(0);
      setFound({ q: "", items: [] });
      queueMicrotask(() => input?.focus());
    } else {
      clearTimeout(timer);
      seq++;
      if (opener instanceof HTMLElement) {
        opener.focus();
        opener = null;
      }
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
    } else if ((e.key === "ArrowDown" || e.key === "ArrowUp") && e.altKey) {
      e.preventDefault();
      setSel(moveSection(shown(), q(), sel(), e.key === "ArrowDown" ? 1 : -1));
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "PageDown" || e.key === "PageUp") {
      e.preventDefault();
      setSel(moveSelection(sel(), n, e.key === "ArrowDown" ? "down" : e.key === "ArrowUp" ? "up" : e.key === "PageDown" ? "pageDown" : "pageUp"));
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
            aria-autocomplete="list"
            aria-activedescendant={shown()[sel()] ? `pal-${shown()[sel()]?.id}` : undefined}
            aria-label={t("palette.title")}
            placeholder={t("palette.placeholder")}
            value={q()}
            onInput={(e) => {
              setQ(e.currentTarget.value);
              setSel(0);
              ask(e.currentTarget.value);
            }}
          />
          <ul id="palette-list" role="listbox" class="plain" aria-label={t("palette.title")}>
            <For each={shown()} fallback={<li class="muted pal-none" role="status">{t("palette.none")}</li>}>
              {(i, idx) => (
                <>
                  <Show when={heads().has(idx())}>
                    <li class="pal-head" role="presentation" aria-hidden="true">{i.group}</li>
                  </Show>
                  <li id={`pal-${i.id}`} role="option" aria-selected={idx() === sel()} class={idx() === sel() ? "pal-item sel" : "pal-item"} onMouseMove={() => setSel(idx())} onClick={() => choose(i)}>
                    <span class="pal-name">{i.label}</span>
                    <Show when={i.detail}><span class="pal-detail">{i.detail}</span></Show>
                    <span class="pal-kind">{i.kind}</span>
                  </li>
                </>
              )}
            </For>
          </ul>
        </div>
      </div>
    </Show>
  );
}
