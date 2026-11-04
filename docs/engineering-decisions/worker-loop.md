# Worker agent loop (experiment X1, decision D90)

## Target experience
The agent that does the work runs where it can do no harm, survives a crash without losing or repeating work, and shows the person every step, refusal and result.

## References
goose-agent 0.1.0-alpha.13 (Apache-2.0, crates.io; local clone commit 71dd9a9): `machine.rs`, `operation.rs` and the `tool.rs` provider trait read in full; `inference.rs`, `subagent.rs`, skills and the tests only surveyed. OpenHands' event store and Codex' resume were surveyed earlier (not re-read for this decision).

## What goose-agent offers
An event-sourced state machine: steps (operations and an inference step) produce effects, a runtime persists them and reloads the session, cancellation leaves unanswered tool calls with a synthetic "interrupted" response, and a `ToolProvider::call(session, request_id, call, emit)` seam fits our broker. It builds in about 30 seconds.

## Decision
Do not embed it for Milestone I. Costs measured: it requires rustc 1.94.1 (forced a toolchain bump), brings `rmcp`, `agent-client-protocol`, MCP result types and its own message model with metadata and visibility, and is an alpha API; the worker needs only a model call, a tool call and a checkpoint. A small loop (`kk-worker`, about 200 lines) keeps the same behaviours that matter: state is saved after every step; a crash resumes from the saved conversation; calls that had no answer are issued again under the same request id (read-only tools are repeatable and effect tools resolve through the effect ledger instead of running twice); cancellation and a turn limit save state first; a refusal is shown to the model and a fatal refusal (expired, budget, replay, no eligible route, policy error) ends the run. Revisit goose-agent when Milestone II needs subagents, skills and compaction. The toolchain stays at 1.94.1.

## Results (VERIFIED_BY_RUN)
- `cargo test -p kk-broker --test worker_loop`: 7 tests with a scripted model (SIMULATED) against the real broker: a full review with a labelled finding, crash then resume, refusals, turn limit and cancellation with resume, a hostile README that the model obeys, bad checkpoints, restore into a weaker session.
- Finding while writing the resume test: restoring a saved conversation into a fresh session started with a clean label, which would launder everything the conversation had seen. Checkpoints are now stored per job together with the taint at save time; loading one absorbs that taint into the new step and is refused if it exceeds the new worker's clearance (test w07).
- Sandboxed run on Linux: see the Linux results in TESTING.md (the real `kk-worker` binary and a hostile script).

## Not covered
Real-model behaviour (only the scripted route is used here; the local-model run is separate); concurrent steps; conversation compaction for long reviews; effect tools through the ledger; parallel tool calls in one turn beyond sequential execution.
