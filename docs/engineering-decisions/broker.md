# Broker (contract K1, tests B01-B09 and new B10-B15)

## Target experience
A worker, however clever or hostile, has exactly one way to touch anything: a short list of typed requests, each decided by policy and by the kernel's own record of what the worker has read. A refused request explains itself without leaking what exists.

## References
Contract K1 (`ARCHITECTURE/ARCHITECTURE_INTERFACE_CONTRACTS.md`); IronClaw's rule that the loop is not the perimeter and the sealed-witness idea (`ironclaw-kernel.md`); Quest's dispatch gate (read in earlier rounds).

## Approach (`kk-broker`, layer 3)
One JSON request per line, strict parsing (unknown operation or field is refused), strictly increasing request ids per session, an allowlist of operations per plan, budgets for calls, bytes read, bytes written and model tokens. Operations: read, write, model completion, tool call, checkpoint, content-free log note, release proposal. Every read and write consults the Cedar policy (`kk-policy`) and then the kernel (`kk-kernel`), which computes the label from its own read log and ignores the worker's `derived_from_hint`. Model calls pick a route through the no-fallback `RouteSet`, then the policy; model output taints the step as untrusted. Built-in tools (`list_files`, `read_file`, `search_text`) read only through the same checks; a file the worker may not read looks exactly like a missing file (same error), is absent from listings, and never taints the step. Every request, including refusals, is appended to an audit list.

## Results (VERIFIED_BY_RUN, `cargo test -p kk-broker`)
15 tests pass: clearance and taint on read; lying hint cannot lower a label; no eligible route is an error not a fallback, and a device route works; tool grants and unknown tools; no file-existence oracle; replay and decreasing ids; seven malformed or oversized request shapes; operations outside the plan; call, read and write budgets; closed and unknown sessions; hostile README text is data and raises the untrusted flag; release proposals by role and against real handles; per-job handle spaces; bounded checkpoints that fail without changing state; audit of ok and refused calls. Mutation check: 7 of 8 mutations caught (replay check, plan allowlist, call budget, model-output taint, file read check, listing filter, unknown-field rejection); the eighth (route chosen without regard to taint) is an equivalent mutant today because the device route is always eligible, and the policy layer is what blocks tainted data on other classes.

## Found while testing
`#[serde(flatten)]` combined with `deny_unknown_fields` rejects every valid request in serde; the envelope is now parsed explicitly (`parse_line`).

## Not covered
The transport (worker stdin and stdout) and the sandboxed worker; effect tools through the effect ledger (the request id is carried but unused until a first effect tool exists; non-read-only tools need a human approval that no session can have yet); the evidence-log append of audit events; per-step expiry by wall clock; plan compilation; fuzzing the request parser with mutated JSON; covert-channel budgets (R17) beyond byte and call counts.
