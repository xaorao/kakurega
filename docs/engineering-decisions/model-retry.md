# Retrying a model call that failed for a passing reason (ledger E-A5)

## Target experience
A local model that is starting, restarting or overloaded does not turn a review or a chat message into a failure. A refusal, a bad request or an over-long conversation fails at once, with its own reason.

## Reference and reading depth
goose (Apache-2.0, commit 71dd9a9): FULL read of `crates/goose-provider-types/src/retry.rs` (three retries, 1 s doubling to 30 s, jitter 0.8 to 1.2, retry only server, network and rate-limit errors, provider-supplied delay honoured) and the error type in `errors.rs` (first 150 lines). `goose-providers/src/http_status.rs` lines 1-140 read later (PART; 927 lines and `ollama.rs` NOT READ).
- Discoveries from that reading, each judged: (1) honour a server's `Retry-After` (seconds or HTTP date, also the body's `retry_after_seconds`), with a hard cap and a past date meaning retry now, and ignore NaN, negative or absurd values: ADOPT when the NEAR AI Cloud route exists (a cloud model rate-limits; the local Ollama route sends no such header, so nothing to apply now); (2) strip credentials and query strings from a URL before it enters an error message: ADOPT with the cloud route (our local route's errors carry no URL, and the fetcher's already do not carry credentials); (3) recognise a context overflow from the body's `n_prompt_tokens` greater than `n_ctx` as well as by message: our `overflow_message` reads llama.cpp's sentence only, so a differently worded overflow would be retried or reported as a bad response: checked against a real Ollama (ollama version is 0.40.14), model qwen2.5:1.5b, `num_ctx` 1024 and `truncate: false`: HTTP 400 with `{"error":"request (6030 tokens) exceeds the available context size (1024 tokens), try increasing it"}`, the exact sentence our `overflow_message` reads, so the local route is VERIFIED for this version; the body-token-count form and the NEAR AI Cloud error shape stay NOT VERIFIED until the cloud route exists.

## Taken and changed
- Taken: bounded retries with doubling, jittered waits; only "not now" failures retried; never a refusal, bad request, authentication or context error.
- Changed: goose's default also retries 4xx request failures unless asked not to; ours never does. Two retries (0.5 s, then 1 s, capped at 4 s) rather than three from 1 s, because the owner is waiting on a model on their own computer. A call that ran out of time is not retried at all (the model was working; a second try would wait as long again). A streamed reply is retried only before the first byte: once text has reached the screen a broken stream is an error, never a second attempt that would show it twice. The final error says how many tries were made.
- `kk_route::retry` (pure, tested with skipped waits); `RouteError::Busy` (429, 502, 503, 504) and `TimedOut` are new; `OllamaRoute::post_chat` serves both `complete` and `stream`.

## Evidence
kk-route rt01–rt05 (growing waits, exhaustion, final failures untouched, no retries, caps and jitter) and rt10–rt15 against a scripted server (busy twice then answers, dropped connection, stays busy and says three tries, 400/401/404/500 and a too-long conversation asked once, a timeout asked once and quickly, a stream retried only before it begins). 8 mutants caught.

## Not covered
The NEAR AI Cloud route (not built yet); the embedding and attestation calls (separate code, not retried); a retry budget across a whole review (each call retries on its own).
