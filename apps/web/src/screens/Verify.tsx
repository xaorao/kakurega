import { createSignal, For, Show } from "solid-js";
import { b64urlDecode, verifyCapsule, verifyShare } from "@kakurega/verifier";
import { rich, t, type Key } from "../i18n";
import { visible } from "../visible";

function reasonText(code: string): string {
  const key = `reason.${code}` as Key;
  const text = t(key);
  return text === key ? code : text;
}

// The refusals only a share can have, in the standalone page's words, until they have keys of their own in every language.
const SHARE_REASONS: Record<string, string> = {
  share_not_json: "The file is not a share.",
  share_not_canonical: "The file was changed after it was made (its bytes are not in the one allowed form).",
  share_shape: "The file does not have the shape of a share.",
  share_too_large: "The file is too large to be a share.",
  not_in_release: "A file in this share is not part of the signed release: it was changed, or was never in it.",
  leaf_shape: "A file entry in this share is malformed.",
  leaf_place: "A file entry in this share claims a place it cannot have.",
  leaf_order: "The file entries of this share are not in their one allowed order, or one is repeated.",
  leaf_count: "This share carries no file, or too many.",
  disclosure_shape: "The signed statement in this share does not have the shape it must have.",
};

const NOT_SHOWN = "Not shown here: it is not plain text, or it is long.";

/** A shared file's text when it is plain text of a readable size; otherwise null and only its size is shown. */
function readable(bytes: Uint8Array): string | null {
  if (bytes.length > 20000) return null;
  try {
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(text) ? null : text;
  } catch {
    return null;
  }
}

/** A share says what it is in its first field; the check that follows decides whether it is a valid one. */
function isShare(bytes: Uint8Array): boolean {
  try {
    return JSON.parse(new TextDecoder().decode(bytes))?.format === "kakurega-share";
  } catch {
    return false;
  }
}

type Checked = { kind: "capsule"; r: any } | { kind: "share"; r: any };

export function Verify() {
  const [result, setResult] = createSignal<Checked | null>(null);
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
        setResult(null);
        setError(t("ver.pinError"));
        return;
      }
      pinned = [raw];
    }
    const bytes = last;
    try {
      if (isShare(bytes)) setResult({ kind: "share", r: await verifyShare(bytes, { pinned }) });
      else setResult({ kind: "capsule", r: await verifyCapsule(bytes, { pinned, now: Math.floor(Date.now() / 1000), recipient: expected().trim() || null }) });
    } catch (e) {
      // the checker stopped on something it did not foresee: said in words, and nothing is shown as checked
      setResult(null);
      setError(t("status.unavailable", { why: e instanceof Error ? e.message : String(e) }));
    }
  };
  const load = async (file: File) => {
    setName(file.name);
    setResult(null);
    setError("");
    try {
      last = new Uint8Array(await file.arrayBuffer());
    } catch (e) {
      last = null;
      setError(t("files.error", { name: file.name, why: e instanceof Error ? e.message : String(e) }));
      return;
    }
    await run();
  };
  const drop = (e: DragEvent) => {
    e.preventDefault();
    const f = e.dataTransfer?.files?.[0];
    if (f) void load(f);
  };

  return (
    <main class="verify stack wide">
      <h1>{t("ver.title")}</h1>
      <p class="lead">{rich("ver.lead")}</p>
      <div class="card stack" onDragOver={(e) => e.preventDefault()} onDrop={drop}>
        <label for="file">{t("ver.file")}</label>
        <input id="file" type="file" accept=".kkg,.kks,application/json" onChange={(e) => { const f = e.currentTarget.files?.[0]; e.currentTarget.value = ""; if (f) void load(f); }} />
        <label for="pin">{t("ver.pin")}</label>
        <input id="pin" value={pin()} onInput={(e) => setPin(e.currentTarget.value)} onChange={run} spellcheck={false} autocomplete="off" />
        <label for="rcp">{t("ver.recipient")}</label>
        <input id="rcp" value={expected()} onInput={(e) => setExpected(e.currentTarget.value)} onChange={run} autocomplete="off" />
        <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
      </div>
      <Show when={result()?.kind === "capsule" ? result()!.r : null}>
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
                    {(x: any) => <li><code>{t("ver.receipt", { model: x.model, id: x.completionId, request: x.requestSha256.slice(0, 12), answer: x.responseSha256.slice(0, 12), signer: x.signer.slice(0, 12) })}</code></li>}
                  </For>
                </ul>
              </Show>
              <h2>{t("ver.notEstablished")}</h2>
              <ul class="plain">
                <For each={r().notEstablished}>{(n: string) => <li>{n}</li>}</For>
                <Show when={(r().receipts ?? []).length > 0}><li>{t("ver.notHardware")}</li><li>{t("ver.notReceiptClaims")}</li></Show>
                <li>{t("ver.not1")}</li>
                <Show when={!r().signerPinned}><li>{t("ver.not2")}</li></Show>
              </ul>
              <p class="muted">{rich("ver.assurance", { level: r().bindingLevel, route: r().route })}</p>
            </Show>
          </section>
        )}
      </Show>
      <Show when={result()?.kind === "share" ? result()!.r : null}>
        {(r) => (
          <section class="card stack" id="share-result" aria-live="polite">
            <Show when={r().valid} fallback={<><h2 class="bad">{t("ver.notValid")}</h2><p>{SHARE_REASONS[r().reason] ?? reasonText(r().reason)}</p><p class="muted">{rich("ver.code", { code: r().reason })}</p></>}>
              <h2 class="ok">{t("ver.valid")}</h2>
              <p class="muted">{name()}</p>
              <h2>{t("ver.established")}</h2>
              <ul class="plain wrap">
                {/* the checker's own sentences: the signer, that each shared file is one of the release's files, and the capsule's digest */}
                <For each={r().established}>{(n: string) => <li>{n}</li>}</For>
                <Show when={r().signerPinned}><li>{t("ver.est4")}</li></Show>
              </ul>
              <h2>{t("src.files")}</h2>
              <For each={r().files}>
                {(f: { name: string; bytes: Uint8Array }) => {
                  const text = readable(f.bytes);
                  return (
                    <section class="stack shared-file" aria-label={f.name}>
                      {/* a file name keeps its own letters: not a heading, whose style changes them to capitals */}
                      <p class="shared-name"><strong><code class="wrap">{visible(f.name)}</code></strong> <span class="muted">{t("rel.bytes", { n: f.bytes.length })}</span></p>
                      {/* shown as text only: nothing in a shared file is ever treated as markup */}
                      <Show when={text !== null} fallback={<p class="muted">{NOT_SHOWN}</p>}>
                        <pre tabindex="0">{visible(text ?? "")}</pre>
                      </Show>
                    </section>
                  );
                }}
              </For>
              <h2>{t("ver.notEstablished")}</h2>
              <ul class="plain">
                <For each={r().notEstablished}>{(n: string) => <li>{n}</li>}</For>
              </ul>
            </Show>
          </section>
        )}
      </Show>
    </main>
  );
}
