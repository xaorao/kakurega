# Kernel-computed taint (experiment X14, contract K1, tests LB01-LB06)

## Target experience
A worker can be wrong, hostile or merely sloppy and the output still carries the right label, so a release decision never rests on what the worker claims.

## References
Contract K1 in `ARCHITECTURE/ARCHITECTURE_INTERFACE_CONTRACTS.md`; CaMeL-style control of derived data; IronClaw's rule that the loop is not the perimeter (`crates/kernel/AGENTS.md`); Quest's binary private/public split, which has no label inheritance.

## Approach
`kk-types::label` defines the lattice: level (Public < Internal < Confidential < Restricted), compartments, integrity (Trusted < Untrusted) and an unknown-sensitivity flag treated as Restricted. `join` is commutative, associative, idempotent, never lowers any component (property tests). `kk-kernel::taint` keeps per-job artifact stores keyed by tenant and job; a step session records every read and every absorbed input (tool output, memory recall, model input), and the label of any written artifact is the join of that taint. The `derived_from_hint` parameter is ignored for labelling. Clearance is checked before the taint is taken. A session has no operation that lowers a label.

## Results (VERIFIED_BY_RUN, `cargo test -p kk-types -p kk-kernel`)
- LB01 to LB06 and a clearance-ordering test pass; six lattice property tests pass.
- Mutation check, run by hand and restored: (1) labelling from the declared parents instead of the read log fails 5 of 7 tests; (2) reads not tainting fails 5 of 7; (3) skipping the clearance check fails the clearance test.

## Not covered
Release pipeline declassification (LB06 beyond "no API exists") waits for the release component; persistence of labels in the vault; concurrent steps on one job; labels of binary artifacts derived by external tools beyond the join rule.
