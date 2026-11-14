import { createSignal } from "solid-js";

/** Whether the model on this computer cannot answer right now (its service is not running, or the model is not installed).
 *  Read from the readiness checks by the app; every place that says "on this computer: nothing leaves" looks here first. */
export const [modelDown, setModelDown] = createSignal(false);
