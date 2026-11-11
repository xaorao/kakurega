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
| Registering a capsule (needs the capsule key, held by the service) | BUILD: the owner-only service call `POST /admin/jobs/{id}/chain-ticket` signs the registry's binding message, built by the service itself from a capsule it delivered and that still verifies under its own key, for one account and at most one order; `kaku chain register JOB` shows exactly what becomes public and what does not, asks for `yes` at a terminal (or `--yes`), then sends it | `kk-service` `admin_chain_ticket`, `kk-evidence/src/registry.rs`, `kk-cli/src/chain.rs` |

## Evidence
- `cargo test -p kk-chain`: ch01 to ch10 (the platform id read from a real saved quote; malformed certificates; signed objects with braces and quotes inside text; the Borsh layout byte for byte; reports without a key or with an implausible quote; key files; a readable key file refused; mainnet and bad inputs refused before any network use; a refusal reported in the contract's words). 11 of 11 mutants caught (one survived until a test for an unknown platform-id form was added).
- `kk-cli` cc01 to cc05: dates from chain times, control characters removed, amounts in NEAR, bad commands refused with exit 2.
- LIVE on NEAR testnet (VERIFIED by run): the client test recorded `Qwen/Qwen3.8-27B`'s key in transaction `DVMmWxN2SSHiYWttwmzFtqFcbRCENyzXop5eKBRtqmCa` (185.8 Tgas) and read it back; as a user, `kaku chain attest --model Qwen/Qwen3.8-27B --key-file FILE --account NAME` recorded it again in `54m4rq5w4cjgk9sYs5mWUax1AW5SUn6Jwr4dCqczd3qn`; `kaku chain hardware KEY` shows the record with its limits (exit 0) and an unknown key as not recorded (exit 3); `kaku chain order 0` shows the paid order; `kaku chain capsule FILE` reports a valid unregistered capsule as not registered (exit 3) and looks nothing up for an invalid one (exit 4).
- Found by running it: `cargo near` writes key files readable by every user of the machine; the client refused them until `chmod 600`. And a capsule names its signer by a key id (a hash), while the registry is keyed by the key itself, so the lookup first failed for every capsule; it now takes the key from the capsule's own key list.

## Registering a capsule: evidence
- The message format is asserted with the same literals on both sides: `kk-evidence` rb01 and the contract's rg19, and a third computation (Python) of the receipts digest agreed with both.
- `kk-service` sv01: the ticket's message is the registry's own for this capsule, account and order, and the capsule key's signature over it verifies; a plain registration is a different message; names that are not accounts (including one carrying `:order:9`) get nothing signed; an extra `message` field is refused; a paired browser session is not the owner's terminal; the signed log records that a ticket was made, not for whom; once the delivered file no longer verifies, nothing is signed.
- LIVE as a user (`tests/e2e/chain_register.py` with a throwaway testnet account, 15 of 15, VERIFIED by run): a review run, approved and delivered by the real binaries; before registration the chain says not registered; without a terminal and without `--yes` nothing is published; `kaku chain register` shows what becomes public and registers; `kaku chain capsule FILE` then names the account and time; a second registration is refused in the contract's words; an altered capsule is not looked up; a review that delivered nothing has nothing to register. Without the two environment variables the script runs its offline checks and prints SKIPPED for the rest.
- Decision recorded: publishing a capsule's digest and receipts is a new disclosure (to everyone, for good), separate from the release to its recipient. It is the owner's act at the owner's terminal, with the list of what is published shown first; it is not routed through the release approval, and no agent, browser session or MCP client can ask for it.

## Paying an order from another chain: a price, never an offer to pay
- Docs read first: the NEAR Intents 1Click quote and token references (the `near-intents` skill: `api-quote.md`, `api-tokens.md`; upstream `docs.near-intents.org`). Facts confirmed by live dry calls: a quote with `customRecipientMsg` is accepted and echoed back; the service answers 400 for a recipient that does not exist on NEAR mainnet; 1Click has no test network.
- Built: `kk-chain/src/intents.rs` asks only for a dry quote (`dry` is a constant, not a parameter) that would deliver an exact amount of USDC on NEAR to the registry with the order's terms as the transfer's message, which is what the contract's `ft_on_transfer` reads to open an order. An answer is refused when it carries a deposit address, is not confirmed dry, or did not keep the terms. `kaku chain quote` prints the price in words and says that nothing is sent.
- Evidence: in01 to in03 (6 of 6 mutants caught; a seventh planted change turned out to be equivalent to the original); `kk-cli` cc06 and cc07 (amounts, times, malformed terms refused before any network use). LIVE (VERIFIED by run): 25 USDC at the registry costs about 25.2555 USDC from Solana, about 20 seconds; 100 USDC costs about 0.00137 BTC, about 805 seconds; an unknown coin is named as unknown (exit 2).
- Not done, and not to be done without the owner's word: the registry on NEAR mainnet and a real payment. `intents.near` stood in as an existing recipient for the live dry quotes.

## Not covered
- Opening an order and delivering for one from the command line (`Chain::open_order` exists and is untested live; delivery needs the capsule key).
- A proxy setting for these requests; a retry when NEAR's RPC is busy.
- The check that an existing record made by another account is reported (seen only in code).
