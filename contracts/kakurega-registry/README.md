# Kakurega capsule registry

A NEAR testnet contract that makes three public statements about a Kakurega proof capsule, each checked on chain.

- **When and by whom.** A NEAR account recorded the capsule's SHA-256 digest, with a signing key, at a block time. The first registration of that digest under that key wins and is never overwritten. Entries are keyed by digest plus signer, so a different key registering the same digest first does not take the capsule's place; a verifier compares the entry's signer with the key inside the capsule.
- **Whose key.** The registering account controls the key that signed the capsule. It proves this with an Ed25519 signature over `kakurega-register-v2:<digest>:<receipts digest>:<account id>`. The message names the account, so another account cannot replay it, and covers the receipts, so they cannot be swapped. The contract can show that the registrant controls A key; that the key is the one that signed the capsule is for the verifier to check.
- **What a model signed.** Each receipt, a model enclave's signature over `MODEL:SHA256(request):SHA256(answer)`, is verified with the runtime's own Ed25519 check.

It does not say that a receipt's signer key belongs to attested confidential hardware (the hardware quote is checked off chain by `kaku verify` and the terminal's `/proof`), and it says nothing about whether a claim in the capsule is true. There is no owner, no admin call and no upgrade path.

## Methods

- `new()`: initialises the contract.
- `register(capsule_sha256, capsule_signer, binding_signature, receipts)`: payable. The deposit pays for the storage; the rest is refunded.
- `get(capsule_sha256, capsule_signer)`: the entry, or null.
- `total()`: how many capsules are registered.
- `verify_receipts(receipts)`: which receipts verify, in order. A malformed receipt is `false`.
- `binding_message(capsule_sha256, receipts, account_id)`: the exact text the capsule key must sign.

Keys that encode a point of small order are refused everywhere. On the real runtime such a key with an all-zero signature verifies, which would let anyone forge a receipt; the unit-test mock does not show this, the sandbox test does.

## Build and test

```bash
cargo near build non-reproducible-wasm   # needs cargo-near and Rust 1.95
cargo test                               # 18 unit tests, plus one run on a local NEAR sandbox node
```

The receipt check follows the approach of SealedBounty's `tee.rs` (MIT); see `NOTICE.md` at the repository root. The code is written for this contract.
