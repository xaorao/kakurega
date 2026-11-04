# NEAR AI Cloud model attestation (experiment X9a, tests M, AT)

## Target experience
Before any private data goes to a NEAR model, Kakurega checks, itself, that the model runs inside genuine confidential hardware at an acceptable patch level with GPU evidence, and the capsule records what was checked and what was not.

## References
`nearai-inference-sdk` 0.2.1 (MIT, crates.io, repository commit bdd868f read for its API: `cloud_api.rs`, `model.rs`, `types.rs`, `quote.rs`, `attestation.rs`): fetching evidence, Intel TDX quote verification through `dcap-qvl` and Intel's collateral service, NVIDIA GPU evidence through NRAS, nonce and signer binding, debug-mode rejection (`attestation.rs` line 52 rejects a quote with the debug attribute set). Live docs: Verification Policy and TLS pages (`docs.near.ai/cloud/verification/...`).

## Decision
Use the official SDK as the verification engine (reuse, MIT, credited in NOTICE) and add the Kakurega policy on top in `kk-near`: accept only an up-to-date TCB by default, require verified GPU evidence by default, optional allow-lists of compose and OS image measurements (a configured list must match and the measurement must exist), every deployment behind a model must pass, and the verified facts are returned as a `RouteAttestation` for the capsule's route report. We do not re-implement quote verification.

## Results
- VERIFIED_BY_RUN, live and keyless (public evidence endpoint and Intel/NVIDIA verification services; no API key used): fresh attestation for `Qwen/Qwen3.8-27B` verified end to end: Intel TDX chain, TCB `UpToDate`, GPU evidence `Verified`, nonce bound, signer `0x835ab53d...aa26`; the Kakurega policy accepted it.
- Same live evidence, tampered: replayed under another nonce, one changed quote digit, a different claimed signer, an edited compose file: all four rejected by the SDK; the untouched control verifies.
- `cargo test -p kk-near`: 3 pure policy tests (TCB not accepted, GPU missing, measurement not allowed or missing, relaxed policy records that GPU was not verified).
- Resolves part of RK-53: model attestation reports are available without an API key (observed on this date's service); the documentation page still says a key is required, so we do not depend on it: the client accepts a key when one exists.

## BLOCKED until the NEAR API key and credits exist (not verified, not claimed)
Signed completion verification (`verify_model_response` and gateway response signatures), E2EE v2 and v3 requests, the `ProviderTee` versus `Gateway` signature kind on a real completion, route report in a real review, TLS binding to the gateway on the same connection, image provenance with sigstore, model quality. Exact evidence needed when the key arrives: one real completion with its signature verified against the attested signer, one E2EE round trip, the route report embedded in a capsule, and the bake-off runs.

## Not covered
An offline recording of the Intel collateral (the verification needs the network); a fake NEAR server for attack tests of the completion path (needs the completion verifier first); the SDK's own tests (upstream).

## Update: the proof is now a product feature (not only a backend check)
- What exists: `POST /api/route/attest` in the service, the "Check a confidential model's hardware proof" panel in the web Chat screen. The owner chooses a model name; Kakurega fetches fresh public evidence with a random challenge, verifies the Intel TDX chain and NVIDIA GPU evidence on this computer through the official SDK, applies the Kakurega policy, and shows the result with what it means and what it does not mean. Only the model name leaves the computer. A refusal (policy or fetch failure) is a 422 with the reason, never a pass; checks are rate limited (3 s).
- Evidence: `sv30` (fake attester: verified, policy refusal, fetch failure, input validation, rate limit, log holds model and outcome only), `sv31` (LIVE, VERIFIED_BY_RUN on 2026-11-04, keyless: the real service verified `Qwen/Qwen3.8-27B` against NEAR AI Cloud, Intel and NVIDIA; an unknown model was refused), browser e2e with both result displays and axe.
- Still BLOCKED (needs NEAR API key and credits): signed-answer verification, E2EE completions, binding a real answer to this attested signer, and showing that binding in the capsule. The panel states this limit. When the key exists: run the same panel followed by a real completion, verify its signature against the attested signing key, and record both in the capsule route evidence.
- Opportunities to evaluate next (not yet read or built): NearClaw's approach to showing proof to the user, NEAR Intents for paying for inference, and publishing capsule digests to NEAR as a public timestamp. Each needs a source read before any design.
