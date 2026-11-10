# The NEAR route: a model on confidential hardware that gives no answer until both proofs verify (NEAR loop, step 2)

## Target experience
1. Private data can be sent to a model that runs on NEAR AI Cloud's confidential hardware, and the owner and any recipient can see why that was safe: the hardware proof checked out before the request, the request travelled encrypted to the model's own key, and the answer came back signed by the model's hardware over exactly the bytes exchanged. If any part fails, nothing is given to the owner.
2. Strongest references: NEAR's official inference SDK (this route is built on it), and the rival entries' verifiers that separate the gateway's signature from the model's.
3. Approach: `kk_near::route::NearRoute` implements the kernel's `Route`. The kernel, not the route, decides which data it may receive (`route_ceiling`: restricted only because the reply key is bound).
4. Quality criteria: no request to an unverified deployment; a gateway-signed answer is not accepted as the model's; a tampered or wrongly signed answer is held back; every answer leaves a receipt; errors are named; nothing is retried that could double a charge.

## Reference and reading depth
- `nearai-inference-sdk` v0.3.0 (MIT, NEAR AI) commit ff59105, in `private-ai/inspiration/inference-sdk/rs`: `src/e2ee.rs` lines 1 to 541 (the request binding: the reply key and the field's place are bound into each encrypted field, so a held ciphertext cannot be replayed to another reader) and `src/inference.rs` lines 1 to 1049 (the session: gateway then model verification, pinned TLS to the attested gateway, the response record, `verify_response`, the cache) read in full; the SDK's own tests (`tests/inference_client.rs` 330 lines of the fake gateway and `tests/support`) read for the fixtures. NOT READ: `ohttp.rs`, `systemone.rs`, the Go, Python and JavaScript SDKs, the unit tests inside `e2ee.rs` and `inference.rs`.
- What changed from 0.2.1 (diff of the crates): E2EE v3 request binding (`e2ee.rs` 568 to 1033 lines), tighter response-shape checks, pinned TLS for the evidence and signature calls, a shared HTTP client for them.
- Live check of the upgrade, keyless, against the real NEAR endpoints: the four attestation tests of `kk-near` pass on 0.3.0 (VERIFIED by run).

## Built
- `kk-route`: `RouteError::NotVerified`, a `receipt` on `RouteEvidence`, `neutralised` (the one text clean-up every model route applies), `Route::reply_key_bound` and the kernel's `route_ceiling` (see `near-attestation.md`).
- `kk-near::route::NearRoute`: before the request, `verify_deployment` (gateway attestation with a fresh challenge and TLS binding, model attestation with the product's policy: up-to-date patch level and a verified GPU report by default); the request body is OpenAI-shaped, with tools, and encrypted end to end by the SDK; after the answer, `verify_response` and the check that the signature kind is the model's (`provider_tee`), not the gateway's; the answer's fields are decrypted by the SDK; tool-call arguments are read as data; a receipt (completion id, signature kind, signer, signed text, signature, deployment: gateway and model patch level, GPU check, compose and OS image hashes, the time) is kept for `evidence()`.
- HTTP statuses are named (401 and 403, 402, 404, 413, 429 and 5xx busy); a failed proof is `NotVerified` and is never retried.
- The layer of `kk-near` moved from 1 to 2 because it now depends on `kk-route`.

## Evidence (all SIMULATED, never claimed as live)
- `kk-near/tests/near_route.rs` nr01 to nr08 against a fake gateway (`tests/support/mod.rs`, fixtures adapted from the SDK's MIT-licensed tests, credit in NOTICE.md): a decrypted answer with a provider-signed receipt and nothing private in the wire bytes; a gateway-signed answer held back; a tampered answer and a wrong signer held back; a deployment with a bad report gets no request at all; named service errors; tool calls; the kernel offers restricted data only while the route verifies; no route without end-to-end encryption or with a bad model name. 10 of 10 mutants caught (counted from test failures only, compile errors excluded).
- Unchanged and green after the change: kk-route, kk-broker, kk-worker, kk-review, kk-tui, kk-cli and kk-service suites.

## BLOCKED until the NEAR key and credits exist
- A real completion through this route: the signature checked against the attested signer, an end-to-end round trip, the receipt in a real capsule (acceptance rows I-08 and III-08). The key goes in `.secrets/near-ai.env` as `NEAR_AI_API_KEY`; nothing reads it yet.

## Not built yet (next steps of the NEAR loop)
- Choosing this route from `kaku serve` (configuration and the key from the environment), streaming (a stream is verified after it ends), the receipt as a field of the capsule and its checks in both verifiers, and everything after step 2 of the plan in `PROGRESS.md`.
- The fake gateway cannot say anything about real hardware: it reads the report data from the quote string itself.
