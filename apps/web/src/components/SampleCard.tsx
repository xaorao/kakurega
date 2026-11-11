import { createResource, createSignal, Show } from "solid-js";
import { api, ApiError } from "../api";
import { t } from "../i18n";

/** Where another screen finds what the sample card asked it to open with. Read once, then cleared. */
export function takeHint(key: "kk-docs-folder" | "kk-meeting-text" | "kk-meeting-title"): string | null {
  try {
    const v = sessionStorage.getItem(key);
    if (v !== null) sessionStorage.removeItem(key);
    return v;
  } catch {
    return null;
  }
}
const leave = (key: string, value: string) => { try { sessionStorage.setItem(key, value); } catch { /* the screen then opens empty */ } };

/** A made-up project and documents to try everything on, loaded into a folder of their own and removable. */
export function SampleCard(props: { onReview: (folder: string) => void }) {
  const [state, { mutate }] = createResource(() => api.sample().catch(() => null));
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const act = async (fn: () => ReturnType<typeof api.sampleLoad>) => {
    setBusy(true);
    setError("");
    try {
      mutate(await fn());
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t("sample.error"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Show when={state()?.possible}>
      <section class="card stack" id="sample" aria-labelledby="h-sample">
        <h2 id="h-sample">{t("sample.title")}</h2>
        <Show when={state()?.loaded} fallback={
          <>
            <p>{t("sample.lead")}</p>
            <div class="actions"><button type="button" disabled={busy()} onClick={() => void act(api.sampleLoad)}>{t("sample.load")}</button></div>
          </>
        }>
          <p>{t("sample.loaded", { folder: state()?.folder ?? "" })}</p>
          <div class="actions">
            <button type="button" class="primary" onClick={() => props.onReview(state()?.project ?? "")}>{t("sample.review")}</button>
            <button type="button" onClick={() => { leave("kk-docs-folder", state()?.documents ?? ""); location.hash = "#/docs"; }}>{t("sample.ask")}</button>
            <button type="button" onClick={() => { leave("kk-meeting-text", state()?.transcript ?? ""); leave("kk-meeting-title", t("sample.meetingName")); location.hash = "#/meeting"; }}>{t("sample.meeting")}</button>
            <button type="button" class="linklike" disabled={busy()} onClick={() => void act(api.sampleRemove)}>{t("sample.remove")}</button>
          </div>
          <p class="muted">{state()?.note}</p>
        </Show>
        <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
      </section>
    </Show>
  );
}
