# IronClaw kernel crates (experiment X4)

## Target experience
Every privileged effect passes one door; an approval means exactly the bytes the person saw; a crash or a race never produces a second effect or an ambient permission.

## References
IronClaw `crates/` (MIT or Apache-2.0 at the user's choice, commit b0b999d). Read in full for this decision: `crates/Architecture.md` (opening and layers), `crates/kernel/AGENTS.md` (nine stages, sealed mints, fail-closed table, enforced rules), `ironclaw_authorization/src/lib.rs` (grants, lease store, state transitions, fingerprint checks), `ironclaw_host_api/src/approval.rs` (invocation fingerprint). Not read: `ironclaw_capabilities` and `ironclaw_host_runtime` bodies, `ironclaw_processes` journal, `ironclaw_resources`, `ironclaw_turns` (their charters were read; bodies are to be read before building the matching Kakurega parts).

## Verdict per crate
- `ironclaw_host_api` (21k lines): do not import; its vocabulary (tenant, mission, thread, scoped paths) is not ours. Ideas kept: the `Authorized` witness, the invocation fingerprint.
- `ironclaw_authorization`: do not import (needs its filesystem substrate, async runtime and trust crate). Adapt the lease state machine: Active, Claimed, Dispatching, Consumed, Revoked with compare-and-swap, fingerprint check on claim, single winner.
- `ironclaw_approvals`: do not import. Adopt the rule that the decision is persisted before any lease is issued and that a denial is durable and issues no lease.
- `ironclaw_trust`, `ironclaw_resources`, `ironclaw_runtime_policy`, `ironclaw_turns`, `ironclaw_processes`: inform only. Footprint measured: `cargo tree -p ironclaw_approvals` pulls 374 packages including libsql runtime, observability and safety substrates.
- `ironclaw_architecture_tests`: adopt the technique (layer matrix plus a sole-implementor census that scans the whole workspace), not the code.

## What we take and where it lands in Kakurega
1. Sealed `Authorized` witness minted only by the broker fold; consumed once. Enforced by a workspace scan test (AR03: the trait that mints the witness is implemented in exactly one crate, with self-tests that the scan cannot silently stop finding things).
2. Lease state machine with compare-and-swap for exact-bytes release approvals. The approval fingerprint is SHA-256 over the RFC 8785 encoding (D87) of the exact release bytes, recipient, scope, expiry and policy version. IronClaw hashes serde output of a struct and allows floats; ours is stricter.
3. Decision before lease; denial durable.
4. Terminal state written once; result stored before terminal status (effect ledger).
5. Reserve, execute, reconcile for model budget; reservation failure is a denial.
6. Same-layer edges are forbidden outright in AR01 (IronClaw allows 21 inventoried same-layer edges).

## Where we improve on it
- IronClaw's `abort_dispatch_claimed` returns a Dispatching lease to Claimed, which is only right if the effect is known not to have happened. After a crash mid-dispatch we move to UNCERTAIN for non-idempotent effects (effect classes E3 and E4) and never retry automatically.
- Expiry uses wall-clock `Utc::now()`; ours records the clock source and refuses approvals when the clock moved backwards past issue time.
- Approval fingerprints in ours cover bytes the recipient receives, not only the call arguments.

## Not verified
Whether IronClaw's own tests pass on this machine (not run); behaviour of its membrane under concurrent load.
