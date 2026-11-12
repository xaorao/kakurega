import { type JSX, Show } from "solid-js";

/** A proof's outcome, said once and plainly: a large mark (a check in the proof colour, or a cross in the warning colour),
 *  the verdict in a few words, and one line of what it rests on. Used wherever the product answers "did it hold?". */
export function Verdict(props: { ok: boolean; word: string; summary?: string; children?: JSX.Element; id?: string; headingId?: string }) {
  return (
    <div class={props.ok ? "verdict ok" : "verdict bad"} role="status" id={props.id}>
      <span class="verdict-mark" aria-hidden="true">{props.ok ? "✓" : "✕"}</span>
      <div class="verdict-text">
        <h2 class="verdict-word" id={props.headingId}>{props.word}</h2>
        <Show when={props.summary}><p class="verdict-summary">{props.summary}</p></Show>
        {props.children}
      </div>
    </div>
  );
}
