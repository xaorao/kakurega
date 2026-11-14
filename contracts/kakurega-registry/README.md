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
cargo near build reproducible-wasm       # the same, inside the pinned build image (needs Docker on an x86-64 machine)
cargo test                               # 18 unit tests, plus one run on a local NEAR sandbox node
```

The receipt check follows the approach of SealedBounty's `tee.rs` (MIT); see `NOTICE.md` at the repository root. The code is written for this contract.

## Reproducible build

`.github/workflows/contract-reproducible.yml` builds this contract twice inside the image named in `Cargo.toml`
(`[package.metadata.near.reproducible_build]`) and fails unless the two results are the same bytes. The result for commit
`1ba6b29c0ce6b355a85f05a1e8d751fbcf9b2d79` has SHA-256 `991ce451eb453e4745644e6e24d9b1847cc88bb9f3519d1566ac976737730255`
and is deployed on NEAR testnet at `kakurega-registry-1ba6b2.testnet` (deploy transaction
`537xgnKqFUv2YiJhMREMLG8KFj3AUMQ3jw8XzVGfHY8j`). To check it: ask NEAR for the account's code hash, and ask the contract
for `contract_source_metadata`, which names that commit and the build image. On an Apple-silicon Mac the image runs under
emulation and the build did not finish (it stopped making progress twice), so the build is run in CI on an x86-64 machine.
