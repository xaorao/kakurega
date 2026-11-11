import { Show } from "solid-js";
import { t } from "../i18n";

/** What a screen shows while its data is not there: that it is loading, or that it could not be read, with a way to try again. */
export function LoadState(props: { loading: boolean; retry?: () => void }) {
  return (
    <Show when={props.loading} fallback={
      <p class="bad" role="alert">{t("load.failed")} <Show when={props.retry}><button type="button" class="linklike" onClick={() => props.retry?.()}>{t("load.retry")}</button></Show></p>
    }>
      <p class="muted" role="status">{t("review.loading")}</p>
    </Show>
  );
}
