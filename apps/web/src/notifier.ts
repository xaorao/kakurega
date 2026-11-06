import { browserEnv, createNotifier } from "./notify";
import { t, type Key } from "./i18n";

/** The page's one notifier. The text is looked up when a notice is shown, so a change of language applies at once. */
export const notifier = createNotifier(browserEnv(), (kind) => t(`notify.${kind}` as Key));

export function notifyState(): "unsupported" | "denied" | "off" | "on" {
  if (typeof Notification === "undefined") return "unsupported";
  if (Notification.permission === "denied") return "denied";
  return notifier.enabled() && Notification.permission === "granted" ? "on" : "off";
}

/** Must be called from a click: browsers only ask for permission after a gesture. */
export async function turnNotifyOn(): Promise<void> {
  if (typeof Notification === "undefined") return;
  const answer = Notification.permission === "default" ? await Notification.requestPermission() : Notification.permission;
  notifier.setEnabled(answer === "granted");
}
