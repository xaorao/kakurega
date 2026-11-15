import { createSignal, onCleanup, Show } from "solid-js";
import { t } from "../i18n";
import { shortId } from "../ids";

/** A long digest, hash or key: its two ends in mono, the whole value as its title, and a button that copies the whole value.
 *  `what` names it for the button ("the digest", "the SHA-256 of finding-1.txt"). */
export function Ident(props: { value: string; what: string }) {
  const [copied, setCopied] = createSignal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(timer));
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(props.value);
      setCopied(true);
      clearTimeout(timer);
      timer = setTimeout(() => setCopied(false), 1600);
    } catch {
      /* the browser refused the clipboard: nothing was copied, and nothing is said to have been */
    }
  };
  return (
    <span class="ident">
      <code title={props.value}>{shortId(props.value)}</code>
      <button type="button" class="ident-copy" title={t("id.copy", { what: props.what })} aria-label={t("id.copy", { what: props.what })} onClick={() => void copy()}>
        <Show
          when={copied()}
          fallback={
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" aria-hidden="true">
              <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
              <path d="M10.5 3.5v-.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h.5" />
            </svg>
          }
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M3.5 8.5l3 3 6-7" />
          </svg>
        </Show>
      </button>
      <span class="sr" role="status">{copied() ? t("id.copied") : ""}</span>
    </span>
  );
}
