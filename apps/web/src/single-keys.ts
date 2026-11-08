import { createSignal } from "solid-js";

const KEY = "kk-single-keys";

function stored(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

/** Whether single-key shortcuts (a bare printable key such as ?) are on. They are on until the person turns them off. */
const [singleKeys, setSingleKeys] = createSignal<boolean>(stored());
export { singleKeys };

export function chooseSingleKeys(on: boolean) {
  setSingleKeys(on);
  try {
    if (on) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, "off");
  } catch {
    /* the choice still applies for this visit */
  }
}
