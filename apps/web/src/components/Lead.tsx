import { Show } from "solid-js";
import { t } from "../i18n";
import { splitLead } from "../lead";

/** A screen's opening text: what must be read at a glance, the rest one click away, so the screen's work comes first.
 *  A sentence that says what leaves this computer is never folded: the screen passes `tail` or `full` for it. */
export function Lead(props: { text: string; tail?: number; full?: boolean }) {
  const parts = () => splitLead(props.text, props.tail ?? 0, props.full ?? false);
  return (
    <div class="lead-block">
      <p class="lead">{parts()[0]}</p>
      <Show when={parts()[1]}>
        <details class="lead-more">
          <summary>{t("lead.more")}</summary>
          <p>{parts()[1]}</p>
        </details>
      </Show>
    </div>
  );
}
