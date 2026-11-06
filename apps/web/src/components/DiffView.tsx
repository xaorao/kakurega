import { For, Show } from "solid-js";
import { markChanges, parseDiff } from "../diff";

const MARK = { add: "+", del: "-", ctx: " ", hunk: "", file: "", meta: "" } as const;

/** The pieces of a line in order, each saying whether it is part of what changed. */
function pieces(text: string, marks: [number, number][] | undefined): { text: string; changed: boolean }[] {
  if (!marks || marks.length === 0) return [{ text, changed: false }];
  const out: { text: string; changed: boolean }[] = [];
  let at = 0;
  for (const [from, to] of marks) {
    if (from > at) out.push({ text: text.slice(at, from), changed: false });
    out.push({ text: text.slice(from, to), changed: true });
    at = to;
  }
  if (at < text.length) out.push({ text: text.slice(at), changed: false });
  return out;
}

/** A fix shown the way a code review shows it. The sign in front of each line carries the meaning; colour only helps.
 *  The words that differ inside a replaced line are underlined and bold, so they show without colour too. */
export function DiffView(props: { diff: string; label: string }) {
  return (
    <pre class="diff" tabindex="0" aria-label={props.label}>
      <For each={markChanges(parseDiff(props.diff))}>
        {(l) => (
          <div class={`dl ${l.kind}`}>
            <span class="ln">{l.old ?? ""}</span>
            <span class="ln">{l.now ?? ""}</span>
            <span class="sign">{MARK[l.kind]}</span>
            <span>
              <For each={pieces(l.text, l.marks)}>{(p) => <Show when={p.changed} fallback={p.text}><mark class="chg">{p.text}</mark></Show>}</For>
            </span>
          </div>
        )}
      </For>
    </pre>
  );
}
