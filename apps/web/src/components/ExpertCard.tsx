import { hueOf, initials } from "../monogram";

/** One expert as a card: a monogram in a hue that stays the same for that expert, the name and what they do. The card is
 *  the label of its checkbox, so a click, Space or a screen reader chooses it as before. */
export function ExpertCard(props: { id: string; name: string; summary: string; checked: boolean; disabled: boolean; onChange: () => void }) {
  return (
    <label class="rt-expert" data-expert={props.id}>
      <input type="checkbox" checked={props.checked} disabled={props.disabled} onChange={() => props.onChange()} />
      <span class={`monogram hue-${hueOf(props.id)}`} aria-hidden="true">{initials(props.name)}</span>
      <span class="rt-expert-text"><strong>{props.name}</strong><span class="muted">{props.summary}</span></span>
    </label>
  );
}
