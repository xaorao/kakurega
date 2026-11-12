import { Show } from "solid-js";
import { t } from "../i18n";

/** The owner's home folder written as ~, so a screen does not spell out their user name. Only their own home, as the
 *  service reports it (`api.config().home`): another person's folder, or /Users/Shared, is shown as it is, and until the home
 *  is known nothing is shortened. */
let home: string | null = null;
export const learnHome = (p: string | null | undefined) => {
  const h = p && p.startsWith("/") ? p.replace(/\/+$/, "") : "";
  // a home of "/" or "//" would make every path look like it is inside it
  home = h.length > 1 ? h : null;
};
export const shortPath = (p: string) => (home !== null && (p === home || p.startsWith(`${home}/`)) ? `~${p.slice(home.length)}` : p);

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
