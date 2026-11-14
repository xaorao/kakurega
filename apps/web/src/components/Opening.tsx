import { createSignal, onMount, Show } from "solid-js";

/** The opening: the black bar fills the window, draws back to the bar of the mark, and the workspace is there. Shown once
 *  for a tab (not on every reload), never to someone who asked for less motion, and it never takes a click: it is a
 *  picture laid over a workspace that is already usable underneath. */
export function Opening() {
  const [on, setOn] = createSignal(false);
  onMount(() => {
    try {
      if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
      if (sessionStorage.getItem("kk-opened")) return;
      sessionStorage.setItem("kk-opened", "1");
    } catch {
      return; // a browser that keeps nothing simply opens without it
    }
    setOn(true);
    setTimeout(() => setOn(false), 1150);
  });
  return (
    <Show when={on()}>
      <div class="opening" aria-hidden="true">
        <div class="opening-mark"><span class="opening-bar" /></div>
        <div class="opening-name">kakurega</div>
      </div>
    </Show>
  );
}
