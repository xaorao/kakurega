# Effect ledger (experiment X15, contract K15, tests EF01-EF10)

## Target experience
When the machine dies at the worst moment, the person is told the truth: done, not done, or "unknown, here is how to check". Nothing is sent twice by accident, and nothing that costs money or cannot be undone is ever retried without a person.

## References
Contract K15 and decisions D75, D88 (`ARCHITECTURE/`); IronClaw `ironclaw_authorization` lease transitions and its process journal's terminal-once rule (read, rebuilt, not copied; see `ironclaw-kernel.md`); goose-agent's event-sourced replay; Stripe-style idempotency keys.

## Approach
`kk-effects` stores intents in an encrypted vault. States: INTENDED, AUTHORIZED, ATTEMPTED, CONFIRMED, FAILED, UNCERTAIN. Each transition is one transaction and an append-only log row (update and delete are blocked by triggers). The `AUTHORIZED -> ATTEMPTED` step is a conditional update, so a racing second worker loses. Opening as the owner takes an exclusive file lock and converts every ATTEMPTED record to UNCERTAIN (crash recovery); a second owner is refused so recovery can never clobber a live attempt. Per class: E1 retries with the same key until the recorded retention window ends; E2 reads back and retries exactly once only if nothing is found; E3 and E4 never retry automatically and wait for a person (`resolve_uncertain`). Spending and permission changes need a grant that names that kind, and E4 also needs exact amount, recipient and purpose, plus a cool-down before a new attempt. Re-executing a finished intent is a lookup.

## Results (VERIFIED_BY_RUN, `cargo test -p kk-effects`)
- Crash matrix: a real child process is killed (`abort`) at five boundaries (after intent, after authorisation, after the attempt is recorded, after the remote call, after the outcome is recorded) plus a no-crash control, for each of E1 to E4 against a file-backed remote that survives the crash. 24 runs: no duplicate effect anywhere; E1 and E2 end with exactly one effect and CONFIRMED; E3 and E4 end with at most one effect and stay UNCERTAIN when the outcome is unknown, with no automatic retry. Observed rows are printed by the test.
- EF02 replay is a lookup; EF03 unknown outcome per class; EF04 second authorisation and a racing worker (two threads, two connections) are refused; EF06 grant naming; EF07 key window; EF08 single retry; EF10 cool-down and log; terminal-once and append-only log.
- Mutation checks, each removed by hand and restored: skipping crash recovery, automatic E3/E4 retry, finishing a terminal state, dropping the owner lock, authorising without the state check. All five were caught (the third only after a unit test was added, because the first mutation run exposed that no test reached that guard).

## Not covered
EF11 (a lease stuck in Dispatching) waits for the approval lease component; EF05 against a real service; EF09 reconciliation options shown to a user (UI); concurrent owners on different machines; clock rollback handling (only an injectable clock exists); remote error classification beyond delivered, rejected and unknown.
