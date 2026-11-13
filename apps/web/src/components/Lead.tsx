import { Show } from "solid-js";
import { t } from "../i18n";

/** Where a screen's opening text is cut: after its first sentence, in either language. */
export function splitLead(text: string): [string, string] {
  const m = /^(.*?[.!?।])\s+(\S[\s\S]*)$/.exec(text.trim());
  // a first sentence that is most of the text is not worth folding the rest away for
  const [first, rest] = [m?.[1] ?? "", m?.[2] ?? ""];
  if (!first || rest.length < 60) return [text.trim(), ""];
  return [first, rest];
}

/** A screen's opening text: one sentence to read at a glance, the rest one click away, so the screen's work comes first. */
export function Lead(props: { text: string }) {
  const parts = () => splitLead(props.text);
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
