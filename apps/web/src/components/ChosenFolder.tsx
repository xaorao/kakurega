import { Show } from "solid-js";
import { t } from "../i18n";

/** The folder home written as ~, so a screen does not spell out the owner's user name. */
export const shortPath = (p: string) => p.replace(/^\/(Users|home)\/[^/]+(?=\/|$)/, "~");

/** The folder the owner has chosen: its name with a check, and where it is (the home folder written as ~, the whole path
 *  in the title). */
export function ChosenFolder(props: { path: string; loading: boolean }) {
  return (
    <p class="chosen" aria-live="polite">
      <Show when={props.path} fallback={<span class="muted">{props.loading ? t("new.loading") : ""}</span>}>
        <span class="chosen-name">{t("folder.chosen", { name: props.path.split("/").filter(Boolean).pop() ?? props.path })}</span>{" "}
        <span class="path" title={props.path}>{shortPath(props.path)}</span>
      </Show>
    </p>
  );
}
