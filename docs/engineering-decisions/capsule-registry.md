# Capsule registry contract (NEAR testnet; ledger entry for the NEAR loop, acceptance I-08 and III-08 stay BLOCKED on the live key)

## Target experience
1. A judge or recipient holding a capsule can ask the chain three things: who registered this digest and when, does that account control the key that signed it, and did a model enclave sign each receipt in it.
2. References: SealedBounty `contracts/sealed-bounty/src/tee.rs` (MIT, 7a60122, READ IN FULL: signed-text rebuild and `env::ed25519_verify`; also reads the `eth` and `gateway` kinds and a verdict parser, none of which apply here). Antibody `contracts/near-antibody/src/tee.rs`: NOT READ in this step. NEAR docs `smart-contracts/anatomy/environment` (host functions), `testing/integration-test`.
3. Approach: no owner, no admin, no upgrade; first registration wins; the binding signature names the calling account; storage is billed to the caller and the surplus returned.
4. Criteria: replay by another account fails; a forged or altered receipt fails on the real runtime; a weak key cannot forge.

## Decisions
| Capability | Decision | Where |
|---|---|---|
| Receipt signature check | ADAPT (approach only), rebuilt: `MODEL:REQ:RESP`, strict lowercase hex, model name without `:` or control characters, at most 16 per registration | `receipt_holds` |
| `eth` (ecdsa) and `gateway` kinds | EXCLUDE: our route refuses anything but a provider-TEE Ed25519 signature | |
| Verdict parsing, bounty escrow, allowlist of signers, admin | EXCLUDE: not part of a capsule registry; a signer allowlist would need an owner | |
| Capsule-to-account binding | BUILD: Ed25519 over `kakurega-register-v1:<digest>:<account>` | `register` |
| Pay-on-verified-result | NOT BUILT here (parked experiment) | |

## Found by running it on a real runtime
- The unit-test mock accepted nothing odd, but the sandbox node ACCEPTED an all-zero public key with an all-zero signature as valid (a small-order key, the classic universal forgery). Any contract that checks Ed25519 on chain with an unvetted key can be fooled. Fixed: `is_small_order` rejects the seven y-values of every point of order 1, 2, 4 and 8 (sign bit ignored, non-canonical forms included), for receipts and for the capsule signer.
- The mutant that removes this guard SURVIVES the unit tests (the mock does not show the flaw) and is CAUGHT only by the sandbox test. So the sandbox test is part of the gate for this contract.

## Evidence
- 18 unit tests (rg01 to rg18) and 1 sandbox test on a local NEAR node (VERIFIED by run): registration, replay by another account, forged receipt, weak signer, billing under 0.2 NEAR for 1 attached with the rest refunded, first registration wins, the free `verify_receipts` view.
- Mutants: 10 of 10 on the registration and receipt rules caught; 5 more on the weak-key rule, 4 caught by unit tests and 1 (receipts skip the guard) caught only by the sandbox test.
- Wasm 135.5 KB built with `cargo near build non-reproducible-wasm` (sha256 b4e0a8f1...9155 for this source; the reproducible build needs Docker and was not run).

## Not covered
- Not deployed to testnet (needs a throwaway testnet account; no deploy has been made).
- A real NEAR AI receipt has not been registered (needs the account key). The signer key's link to attested hardware is checked off chain, not here.
- Gas cost per call was not measured. The 16-receipt cap is a judgement, not a measured limit.
- The reproducible (Docker) build and the NEP-330 source metadata were not verified.
- Antibody's contract was not read; re-check it before claiming parity.
