# The chain client (`kk-chain`) and `kaku chain`

## Target experience
1. One command has NEAR's own verifier check a model's hardware proof and records its signing key; reading what the chain holds about a model key, a capsule or an order needs no account and says, each time, what the record does not establish.
2. Strongest references: NEAR's `near-api` crate (the official Rust client, used as a dependency, not rewritten); Antibody's client (MIT, e155d3d) `crates/cli/src/tee.rs` lines 84 to 125 and 200 to 242 for fetching Intel's collateral; Aftercall's key rule (a limited key on the user's machine, never held by a server).
3. Approach: a small crate with no other `kk-*` dependency. `collateral` reads the platform id from the quote's certificate, fetches Intel's signed parts and lays them out as the verifier's Borsh `Collateral`; `registry` reads the contract and sends transactions with a key read from a file the owner names. `kaku chain` prints results in words.
4. Quality criteria: no transaction on mainnet from this build; the key is never written, logged or shown, and a key file other users can read is refused; text from the chain cannot carry control characters to the terminal; every failure says which service failed.

## Decisions
| Capability | Decision | Where |
|---|---|---|
| NEAR RPC, transaction signing | REUSE `near-api` 0.8 | `registry.rs` |
| Intel collateral | ADAPT the steps from Antibody, rebuilt: signed objects are cut out of Intel's answer as sent (a brace inside a string does not end the object), every part is size-bounded | `collateral.rs` |
| A model quote with a chosen challenge | BUILD: a plain request to NEAR AI Cloud's public report address (the official SDK picks its own nonce and offers no way to set one, `cloud_api.rs` line 192) | `Chain::model_quote` |
| Mainnet transactions | EXCLUDE in this build (reads only); a mainnet transaction needs the owner's word | `Chain::writable` |
| Registering a capsule (needs the capsule key, held by the service) | OWED: a service call that signs only the fixed binding message for a capsule it released | |

## Evidence
- `cargo test -p kk-chain`: ch01 to ch10 (the platform id read from a real saved quote; malformed certificates; signed objects with braces and quotes inside text; the Borsh layout byte for byte; reports without a key or with an implausible quote; key files; a readable key file refused; mainnet and bad inputs refused before any network use; a refusal reported in the contract's words). 11 of 11 mutants caught (one survived until a test for an unknown platform-id form was added).
- `kk-cli` cc01 to cc05: dates from chain times, control characters removed, amounts in NEAR, bad commands refused with exit 2.
- LIVE on NEAR testnet (VERIFIED by run): the client test recorded `Qwen/Qwen3.8-27B`'s key in transaction `DVMmWxN2SSHiYWttwmzFtqFcbRCENyzXop5eKBRtqmCa` (185.8 Tgas) and read it back; as a user, `kaku chain attest --model Qwen/Qwen3.8-27B --key-file FILE --account NAME` recorded it again in `54m4rq5w4cjgk9sYs5mWUax1AW5SUn6Jwr4dCqczd3qn`; `kaku chain hardware KEY` shows the record with its limits (exit 0) and an unknown key as not recorded (exit 3); `kaku chain order 0` shows the paid order; `kaku chain capsule FILE` reports a valid unregistered capsule as not registered (exit 3) and looks nothing up for an invalid one (exit 4).
- Found by running it: `cargo near` writes key files readable by every user of the machine; the client refused them until `chmod 600`. And a capsule names its signer by a key id (a hash), while the registry is keyed by the key itself, so the lookup first failed for every capsule; it now takes the key from the capsule's own key list.

## Not covered
- A registered capsule shown by `kaku chain capsule` (nothing is registered from a real capsule yet).
- Opening an order and delivering for one from the command line (`Chain::open_order` exists and is untested live; delivery needs the capsule key).
- A proxy setting for these requests; a retry when NEAR's RPC is busy.
- The check that an existing record made by another account is reported (seen only in code).
