import { createSignal, For, Show } from "solid-js";
import { b64urlDecode, verifyCapsule } from "@kakurega/verifier";
import { rich, t, type Key } from "../i18n";

function reasonText(code: string): string {
  const key = `reason.${code}` as Key;
  const text = t(key);
  return text === key ? code : text;
}

export function Verify() {
  const [result, setResult] = createSignal<any>(null);
  const [name, setName] = createSignal("");
  const [pin, setPin] = createSignal("");
  const [expected, setExpected] = createSignal("");
  const [error, setError] = createSignal("");
  let last: Uint8Array | null = null;

  const run = async () => {
    if (!last) return;
    setError("");
    let pinned: Uint8Array[] | null = null;
    if (pin().trim()) {
      const raw = b64urlDecode(pin().trim());
      if (!raw || raw.length !== 32) {
        setError(t("ver.pinError"));
        return;
      }
      pinned = [raw];
    }
    setResult(await verifyCapsule(last, { pinned, now: Math.floor(Date.now() / 1000), recipient: expected().trim() || null }));
  };
  const load = async (file: File) => {
    setName(file.name);
    last = new Uint8Array(await file.arrayBuffer());
    await run();
  };

  return (
    <main class="verify stack wide">
      <h1>{t("ver.title")}</h1>
      <p class="lead">{rich("ver.lead")}</p>
      <div class="card stack">
        <label for="file">{t("ver.file")}</label>
        <input id="file" type="file" onChange={(e) => e.currentTarget.files?.[0] && load(e.currentTarget.files[0])} />
        <label for="pin">{t("ver.pin")}</label>
        <input id="pin" value={pin()} onInput={(e) => setPin(e.currentTarget.value)} onChange={run} spellcheck={false} autocomplete="off" />
        <label for="rcp">{t("ver.recipient")}</label>
        <input id="rcp" value={expected()} onInput={(e) => setExpected(e.currentTarget.value)} onChange={run} autocomplete="off" />
        <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
      </div>
      <Show when={result()}>
        {(r) => (
          <section class="card stack" aria-live="polite">
            <Show when={r().valid} fallback={<><h2 class="bad">{t("ver.notValid")}</h2><p>{reasonText(r().reason)}</p><p class="muted">{rich("ver.code", { code: r().reason })}</p></>}>
              <h2 class="ok">{t("ver.valid")}</h2>
              <p class="muted">{name()}</p>
              <h2>{t("ver.established")}</h2>
              <ul class="plain">
                <li>{rich("ver.est1", { claims: r().claimCount, signer: r().signer })}</li>
                <li>{rich("ver.est2", { files: r().fileCount })}</li>
                <li>{rich("ver.est3", { recipient: r().recipient, expires: new Date(r().expires * 1000).toLocaleString() })}</li>
                <Show when={r().signerPinned}><li>{t("ver.est4")}</li></Show>
              </ul>
              <Show when={(r().receipts ?? []).length > 0}>
                <h2>{t("ver.receipts")}</h2>
                <p class="muted">{t("ver.receiptsLead")}</p>
                <ul class="plain">
                  <For each={r().receipts}>
                    {(x) => <li><code>{t("ver.receipt", { model: x.model, id: x.completionId, request: x.requestSha256.slice(0, 12), answer: x.responseSha256.slice(0, 12), signer: x.signer.slice(0, 12) })}</code></li>}
                  </For>
                </ul>
              </Show>
              <h2>{t("ver.notEstablished")}</h2>
              <ul class="plain">
                <For each={r().notEstablished}>{(n) => <li>{n}</li>}</For>
                <Show when={(r().receipts ?? []).length > 0}><li>{t("ver.notHardware")}</li><li>{t("ver.notReceiptClaims")}</li></Show>
                <li>{t("ver.not1")}</li>
                <Show when={!r().signerPinned}><li>{t("ver.not2")}</li></Show>
              </ul>
              <p class="muted">{rich("ver.assurance", { level: r().bindingLevel, route: r().route })}</p>
            </Show>
          </section>
        )}
      </Show>
    </main>
  );
}
