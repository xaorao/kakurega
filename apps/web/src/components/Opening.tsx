import { Glyph } from "./Glyph";
import { createSignal, onMount, Show } from "solid-js";

/** The opening: the mark lands like a seal, the name is uncovered beside it, and the curtain lifts off the workspace. Shown when
 *  the page is opened (not on a reload), never to someone who asked for less motion, and it never takes a click: it is a
 *  picture laid over a workspace that is already usable underneath. */
export function Opening() {
  const [on, setOn] = createSignal(false);
  onMount(() => {
    try {
      if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
      // only when the page was opened, never on a reload or a step back: the browser says which, so nothing is kept
      const how = (performance.getEntriesByType("navigation")[0] as PerformanceNavigationTiming | undefined)?.type;
      if (how !== "navigate") return;
    } catch {
      return; // a browser that cannot say simply opens without it
    }
    setOn(true);
    setTimeout(() => setOn(false), 1150);
  });
  return (
    <Show when={on()}>
      <div class="opening" aria-hidden="true">
        <div class="opening-mark"><Glyph /></div>
        <div class="opening-name">Kakurega</div>
      </div>
    </Show>
  );
}
