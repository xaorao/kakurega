# Outside test model (owner's decision; a test aid, not a private route)

## Target experience
1. Until the verified NEAR route has its key, the owner can judge how good answers, roundtables and team runs are with a strong hosted model, and is never in doubt that this model is not private.
2. References: the NEAR route in this product (`kk-near`), whose place this one keeps warm; the hosted model's own HTTP interface (Messages API, version 2026-10-21).
3. Approach: one more route behind the same runner, of class `public`. The kernel already gives a public route nothing, so reviews cannot reach it; the service gives it public notes only; every screen names its host. It is switched on only by `kaku serve --route outside --outside-model anthropic:MODEL` with the key in `ANTHROPIC_API_KEY`.
4. Quality criteria: no file of a review reaches it; no note above public reaches it; the key appears in no log, record, error or screen; with the flag absent nothing changes.

## What it does and does not do
- Answers chat, roundtable and team questions, whole (not streamed). Reviews, coding tasks, embeddings and model choice stay on the local model (`review_route` never returns it).
- The request sends the conversation, a reply limit and a medium effort setting; it sends no sampling setting (this API version refuses them) and asks the service to fall back to a sibling model if a safety filter declines. An answer that hit the reply limit is marked as cut; a declined request is said as declined.
- The key is read from the environment only (a key on a command line is readable by other programs), is sent only to `https://api.anthropic.com` or to a stand-in on `127.0.0.1` in tests, and is not part of any `Debug` output or record.
- It lives in `kk-cli` (`src/outside.rs`), not in `kk-route`: the route crate has no TLS on purpose, so a device-only route cannot reach the internet by construction.

## Evidence
- `kk-cli` ou01 to ou04 (which specs, keys and addresses make a route; class and ceiling; the request's shape; reading answers and every other ending) and `rn_the_outside_model_answers_questions_and_is_never_the_route_of_a_review`.
- `kk-service` sv246: on a public route only public notes reach the model, whatever its address says; sv118 unchanged for other routes.
- OBSERVED with the real service and Sonnet: `tests/e2e/team_web.mjs anthropic:claude-sonnet-5-5` (a four-task plan in the lead's own words, all four done, the header reading "Model at api.anthropic.com, not on this computer") and `tests/e2e/roundtable_web.mjs anthropic:claude-sonnet-5-5` (22 of 22, the resolution in its four parts).
- Found with it and changed: experts asked the owner for details they could have assumed, and failed their task at the third question. The task instruction now says to state an assumption and carry on unless the task cannot be done at all.

## Limits
- Not private, by design: the provider sees every question sent while it is on. It must not be used with real confidential work, and it is removed from the default path of every script.
- Whole answers only, so the chat shows nothing until the answer is complete.
- When the NEAR key arrives, `--route near` takes its place with no change to the screens; quality seen here is of this model, not of the NEAR-hosted one.
