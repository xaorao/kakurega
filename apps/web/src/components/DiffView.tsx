import { For } from "solid-js";
import { parseDiff } from "../diff";

const MARK = { add: "+", del: "-", ctx: " ", hunk: "", file: "", meta: "" } as const;

/** A fix shown the way a code review shows it. The sign in front of each line carries the meaning; colour only helps. */
export function DiffView(props: { diff: string; label: string }) {
  return (
    <pre class="diff" tabindex="0" aria-label={props.label}>
      <For each={parseDiff(props.diff)}>
        {(l) => (
          <div class={`dl ${l.kind}`}>
            <span class="ln">{l.old ?? ""}</span>
            <span class="ln">{l.now ?? ""}</span>
            <span class="sign">{MARK[l.kind]}</span>
            <span>{l.text}</span>
          </div>
        )}
      </For>
    </pre>
  );
}
