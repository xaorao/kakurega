# Hosted mode (II-10): what it may promise

## Target experience
The same workspace opened from any browser, without the owner running anything, still private: the owner's files and memory must not be readable by the operator, and the owner must be able to check that.

## The claim ladder (what each deployment lets us say)
1. **Operator-trusted server** (our service on a plain VM): the operator can read everything in memory. We may say "encrypted at rest and in transit", never "private from us". Not acceptable as the privacy product.
2. **Confidential VM / TEE host** (for example Intel TDX): the operator cannot read memory or disk, and the owner can verify the measurement of the code that is running through attestation. We may say "private from the operator, verifiable", but only if the client checks the attestation before sending anything and refuses on mismatch. NEAR documents TEE attestation as proof of the code that ran, the inputs and the output, signed by hardware the operator cannot tamper with (docs.near.org/primitives/off-chain-compute/what-is, read in this session; Intel TDX named there). The attestation chain for our own kernel image has NOT been built or tested.
3. **Local companion** (today): nothing leaves the owner's machine except what the owner approves. The strongest claim, and the default.

## What exists and what does not
- VERIFIED: the model side already uses attested inference (kk-near, real keyless attestation of a NEAR AI Cloud model with tamper and replay refusal).
- VERIFIED: the kernel is a single service binary with pairing, origin pinning, signed approvals and a signed event log, so it is portable to a server.
- NOT BUILT: multi-user accounts (the vault, memory, approvals and logs assume one owner), TLS termination, per-user keys held only inside the TEE, client-side attestation check, a hosted data lifecycle (backup, deletion proof), abuse and cost controls.
- NOT VERIFIED: that our binary runs and attests inside a TEE host. This needs a TDX-capable host or NEAR AI Cloud's confidential hosting, and credentials or credits we do not have (see HUMAN.md).

## Decision
Hosted mode ships only at rung 2 and the product must say which rung it is on. Rung 1 is never offered as private. Build order: (a) per-user isolation inside the service with tests (vault, memory, jobs, approvals, log, approvers); (b) the client-side attestation gate with a fake attestor for tests; (c) real attestation on a TEE host. (a) and (b) can be built and tested without external access; (c) is BLOCKED until a TEE host exists. Until (c) passes, hosted mode must not be described as private.

## Why this order
Isolation bugs are the likeliest hosted-mode failure and they are testable today. An attestation gate tested only against a fake attestor proves the gate refuses, not that real hardware attests, and must be labelled that way.
