# Approval binding (II-12): what proves the owner approved this exact release

## The gap an outside reviewer found
A Touch ID or terminal prompt proves a person was present. It does not prove they approved this recipient, these bytes, this policy, once. Until now the service accepted an approval from anyone holding the local admin token who knew the digest prefix.

## Contract (framework-independent, in the kernel crates)
- Statement: `{approval id, job, full release digest, recipient, policy version, expiry, one-time nonce}` in canonical JSON, signed as a DSSE envelope (type `application/vnd.kakurega.approval+json`) with an Ed25519 approver key (`kk-evidence::approval`).
- The service creates the nonce with the approval. Once any approver key is enrolled, `approve` requires a proof that verifies against an enrolled key and equals the service's own statement byte for byte; a changed recipient, digest, policy, expiry, approval id or nonce voids it. An old proof cannot be replayed on a later approval.
- Changing who may approve (enrol or revoke) also needs a fresh (120 s) signature from an enrolled approver over the action and the key, so the admin token alone cannot add its own key or remove the approver and fall back to the old path. The first enrolment is the trust root (trust on first use through the local admin token).
- Both terminal tools sign: `kaku approve` and the terminal workspace (`kaku`) use the key at `<data dir>/approver.key` (created by `kaku approver enroll`, mode 0600, refused if others can read it) and refuse to send anything when the service requires a proof and no key exists.

## What this does and does not give
- Gives: a compromised web page, a stolen session, or a leaked admin token without the key file cannot approve or change approvers; every approval is bound to its exact release and cannot be replayed.
- Does NOT give: protection against malware running as the same user (it can read the key file); OS-protected key custody; proof of user presence. The key is a file today. The contract does not change when it is replaced by a Secure Enclave or Keychain key behind a native helper that signs only after Touch ID; that helper is NOT built and not verified on hardware. The desktop shells (Electron and Tauri spike) do not sign yet.
- Legacy path: with no approver enrolled the old admin-token approval still works and the service says `requires_proof: false`. Revoking the last approver reopens it, by a signed owner decision.

## Evidence
`cargo test -p kk-evidence approval` (4 tests), `cargo test -p kk-service --test flow sv20` (enrolment, proof required, stranger key, every changed field, replay on another approval, once only, admin-token-only enrol and revoke refused, stale and wrong-action signatures refused, revoked key dead; seven deliberate breaks each failed it), `cargo test -p kk-tui` tu16, `python3 tests/e2e/signed_approval.py` (real binaries, 10 checks).
