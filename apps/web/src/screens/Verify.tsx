import { createSignal, For, Show } from "solid-js";
import { b64urlDecode, verifyCapsule } from "@kakurega/verifier";

const REASONS: Record<string, string> = {
  capsule_not_json: "This file is not a capsule (or it was cut short or has extra data).",
  capsule_not_canonical: "The capsule was re-formatted after it was made. It is rejected because exact bytes matter.",
  capsule_shape: "The capsule has the wrong structure.",
  unsupported_version: "This capsule uses a version this checker does not understand.",
  bad_key: "The capsule lists a signing key that is invalid or unsafe.",
  no_keys: "The capsule lists no signing key.",
  envelope_count: "The capsule has an unexpected number of signed statements.",
  bad_envelope: "A signed statement is damaged.",
  unknown_payload_type: "A signed statement has a type this checker does not accept.",
  signature_count: "A signed statement does not carry exactly one signature.",
  unknown_signer: "A statement is signed by a key that is not listed in the capsule.",
  untrusted_signer: "The capsule is signed by a different key than the one you pinned.",
  bad_signature: "A signature does not match. The capsule or one of its statements was changed after signing.",
  payload_not_canonical: "A signed statement is not in the exact required form.",
  mixed_signers: "The statements are signed by different keys.",
  duplicate_release: "The capsule has two release statements.",
  release_missing: "The capsule has no release statement.",
  release_shape: "The release statement is malformed.",
  expired: "The release has expired.",
  wrong_recipient: "The capsule was made for a different recipient.",
  too_many_files: "The capsule lists too many files.",
  bad_file_name: "A file name is not allowed.",
  duplicate_file_name: "Two files share a name.",
  attachment_missing: "A file named in the signed release is missing.",
  attachment_extra: "The capsule contains a file that the signed release does not list.",
  bad_attachment: "A file is damaged.",
  digest_mismatch: "A file's content does not match the signed digest. It was changed.",
  claim_shape: "A claim is malformed.",
  claim_not_bound: "A claim is not tied to this release.",
  claim_evidence_missing: "A claim points to a file that is not in the release.",
  capsule_too_large: "The capsule is too large.",
};

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
        setError("The pinned key must be a 32-byte public key in base64url.");
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
      <h1>Check a capsule</h1>
      <p class="lead">This page runs entirely in your browser. The capsule is not uploaded anywhere. It checks the signatures, that every file matches what was signed, and tells you what the capsule does <em>not</em> prove.</p>
      <div class="card stack">
        <label for="file">Capsule file (.kkg)</label>
        <input id="file" type="file" onChange={(e) => e.currentTarget.files?.[0] && load(e.currentTarget.files[0])} />
        <label for="pin">Signing key to trust (optional, base64url public key)</label>
        <input id="pin" value={pin()} onInput={(e) => setPin(e.currentTarget.value)} onChange={run} spellcheck={false} autocomplete="off" />
        <label for="rcp">Expected recipient (optional)</label>
        <input id="rcp" value={expected()} onInput={(e) => setExpected(e.currentTarget.value)} onChange={run} autocomplete="off" />
        <Show when={error()}><p role="alert" class="bad">{error()}</p></Show>
      </div>
      <Show when={result()}>
        {(r) => (
          <section class="card stack" aria-live="polite">
            <Show when={r().valid} fallback={<><h2 class="bad">Not valid</h2><p>{REASONS[r().reason] ?? r().reason}</p><p class="muted">Code: <code>{r().reason}</code></p></>}>
              <h2 class="ok">Valid</h2>
              <p class="muted">{name()}</p>
              <h2>Established</h2>
              <ul class="plain">
                <li>The release and its {r().claimCount} claim(s) carry a valid signature from <code>{r().signer}</code>.</li>
                <li>All {r().fileCount} released files match the signed digests and sizes exactly. Nothing is missing or extra.</li>
                <li>Every claim is bound to this exact release. It was made for <strong>{r().recipient}</strong> and expires {new Date(r().expires * 1000).toLocaleString()}.</li>
                <Show when={r().signerPinned}><li>The signing key is the one you pinned.</li></Show>
              </ul>
              <h2>Not established</h2>
              <ul class="plain">
                <For each={r().notEstablished}>{(n) => <li>{n}</li>}</For>
                <li>That any claim is true; only that it was signed and that its evidence files are as signed.</li>
                <Show when={!r().signerPinned}><li>Who holds the signing key. No key was pinned, so the key shown comes from the capsule itself.</li></Show>
              </ul>
              <p class="muted">Assurance level {r().bindingLevel} · route {r().route}. To re-run the evidence yourself, unpack the files and run <code>python3 run-check.py</code>.</p>
            </Show>
          </section>
        )}
      </Show>
    </main>
  );
}
