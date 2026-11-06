# Retrying a model call that failed for a passing reason (ledger E-A5)

## Target experience
A local model that is starting, restarting or overloaded does not turn a review or a chat message into a failure. A refusal, a bad request or an over-long conversation fails at once, with its own reason.

## Reference and reading depth
goose (Apache-2.0, commit 71dd9a9): FULL read of `crates/goose-provider-types/src/retry.rs` (three retries, 1 s doubling to 30 s, jitter 0.8 to 1.2, retry only server, network and rate-limit errors, provider-supplied delay honoured) and the error type in `errors.rs` (first 150 lines). Not read: the per-provider status mapping (`goose-providers/src/http_status.rs`, `ollama.rs`).

## Taken and changed
- Taken: bounded retries with doubling, jittered waits; only "not now" failures retried; never a refusal, bad request, authentication or context error.
- Changed: goose's default also retries 4xx request failures unless asked not to; ours never does. Two retries (0.5 s, then 1 s, capped at 4 s) rather than three from 1 s, because the owner is waiting on a model on their own computer. A call that ran out of time is not retried at all (the model was working; a second try would wait as long again). A streamed reply is retried only before the first byte: once text has reached the screen a broken stream is an error, never a second attempt that would show it twice. The final error says how many tries were made.
- `kk_route::retry` (pure, tested with skipped waits); `RouteError::Busy` (429, 502, 503, 504) and `TimedOut` are new; `OllamaRoute::post_chat` serves both `complete` and `stream`.

## Evidence
kk-route rt01–rt05 (growing waits, exhaustion, final failures untouched, no retries, caps and jitter) and rt10–rt15 against a scripted server (busy twice then answers, dropped connection, stays busy and says three tries, 400/401/404/500 and a too-long conversation asked once, a timeout asked once and quickly, a stream retried only before it begins). 8 mutants caught.

## Not covered
The NEAR AI Cloud route (not built yet); the embedding and attestation calls (separate code, not retried); a retry budget across a whole review (each call retries on its own).
