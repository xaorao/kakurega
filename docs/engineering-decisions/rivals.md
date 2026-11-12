# Two rival entries read in full: what they do better, what we do better, what was done about it

Read for comparison, not for copying. Both are MIT. Reading depth is at the end.

- Veto (`0xdopewilly/veto`, c1416df): a NEAR vault where an AI screen in an enclave can veto an agent's payment.
- Aftercall (`Don-Vicks/aftercall`, a367ec7): a Mac app that records calls, makes notes in NEAR AI Cloud's enclaves, and does the follow-ups.

## Capability by capability
| What a judge would value | Who has it | Here | Decision |
| --- | --- | --- | --- |
| A README that states the problem, the product in a few steps, why NEAR, and a table of things to run by the time you have | Both | Was a long list of features | BUILT: the README was rewritten, with the same honesty about state |
| A judge's guide with exact commands and the output to expect, and a "what it does not do" list | Both | None | BUILT: `docs/JUDGES.md`, every command run before it was written, checked by `scripts/docs_check.py` |
| A table of claims with their code and tests | Both | None | BUILT in the README |
| A state for every feature (live, built and tested, not yet) | Both | Spread over records | BUILT in the README |
| Hardware evidence checked before anything is sent: TDX quote, NVIDIA evidence, key and nonce binding, pinned gateway key, encrypted request | Aftercall | Same checks through NEAR's own verification library, plus the deployment compared with NEAR's releases, which Aftercall lists and does not check | HAVE. Live answers BLOCKED on an account key; the hardware check itself runs live with no key |
| A proof anyone can re-check in a browser without trusting the author's server | Veto reads NEAR from the browser; Aftercall re-checks what the app recorded | The one-file page checks a capsule offline and can read our registry from a public RPC | HAVE. Stronger in one way: the hardware quote is verified on chain by NEAR's own verifier contract, so the record does not rest on what our app wrote (Aftercall's does, and a forged all-green proof is possible there: its `proof.ts` trusts recorded values) |
| Is the deployed contract the code in the repository | Veto (`check-deployed.ts`) | `scripts/check-deployed` | HAVE |
| A benchmark with a baseline, signed results and a note on who wrote the attacks | Veto (100 cases; a plain string rule scores the same as its model, shown by running one) | 168 leak cases with a plain arm, signed, with the failures named; an injection test with real models | HAVE. Ours names its own leaks |
| Every note backed by an exact quote | Aftercall | Meeting notes and document answers keep only quotes found in the file | HAVE. Theirs matches a negated sentence too; so does ours (a quote check says the words are there, not what they mean), and both say so |
| Share one line and nothing else, with a proof | Aftercall (Merkle path) | One finding of a release with a proof | PARTIAL: not for a line of meeting notes. OWED-L |
| Planted instructions in the material | Aftercall sends the transcript unmarked; Veto relies on the model | Lines that read like orders are withheld before the model, files are framed as data | BETTER here |
| What an outside coding agent gets | Aftercall hands the whole meeting to Claude Code or Codex once switched on | Only text the owner read and approved, once, with none of the owner's agent settings | BETTER here |
| A hosted demo that needs no install | Both | None | OWNER'S DECISION (parked by the owner) |
| A short video | Both | None | OWNER'S DECISION (parked by the owner) |
| Payments, caps, escrow on a contract | Both | Orders paid on a verified capsule (testnet; USDC on a local node) | PARTIAL by choice: this entry is about privacy |
| Audio capture and transcription | Aftercall | None (voice was set aside by the owner) | EXCLUDED for now |
| Phone approvals | Aftercall | None | EXCLUDED: needs a native passkey app |

## What each gets wrong, read in the code
- Veto: in its default mode the full email goes on chain; the hosted server holds both the proposing and the screening keys; its enclave check accepts an out-of-date patch level and does not verify NVIDIA's signature; its benchmark is passed by a four-line rule.
- Aftercall: the transcript is not treated as data; a signature failure still saves the notes (marked unverified); measurements are not compared with known-good values; the public verify page trusts values the app recorded.

## Reading depth
- By me: both READMEs in full; Aftercall `docs/how-it-works.md` in full.
- By two helpers, with file and line references, reviewed by me: Veto's contract (`lib.rs`, `screening.rs`, `verdict.rs`, `evm.rs`, `quote.rs`), bench, core pipeline and attestation, guardian and demo server; Aftercall's `packages/core/src/tee/*`, `notes/pipeline.ts`, `proof.ts`, `nearai.rs`, `attest.rs`, the contract's main paths, `agents.rs` in part. NOT READ in either: most screens, most tests' bodies, Aftercall's relay and backend. Nothing of theirs was run except one rule script against Veto's benchmark.
