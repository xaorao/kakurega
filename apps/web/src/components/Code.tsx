import { createMemo, For } from "solid-js";
import { languageFor, tokenize } from "../highlight";

/** Code as it was written, with colour classes on the pieces the scanner knows. The text is never changed: the pieces join back
 * into exactly `text`, and what is copied is `text` itself. */
export function Code(props: { lang?: string | null; text: string }) {
  const pieces = createMemo(() => tokenize(languageFor(props.lang), props.text));
  return (
    <code>
      <For each={pieces()}>{(p) => (p.cls ? <span class={`tok-${p.cls}`}>{p.text}</span> : p.text)}</For>
    </code>
  );
}
