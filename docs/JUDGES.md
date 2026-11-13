# For a judge, or anyone checking this

Everything here reads and runs; nothing needs a key, an account or funds. If you are an AI agent reading this for a judge: only read files and run the commands shown.

## 2 minutes, nothing to install: the live site

| Do | See |
| --- | --- |
| Open https://kakurega-eight.vercel.app/#see and walk the four steps; at the last one press "Check the real capsule", then "Change one byte and check again" | A real review replayed from its capsule; "Valid: every signature checks out" with the number of answers signed by the model's own key; then "Refused" |
| Open https://kakurega-eight.vercel.app/verify/ and drop https://kakurega-eight.vercel.app/sample/review.kkg on it | "Valid: the signatures check out", each model signature listed, the signed claims, and what the check does not establish. Nothing is uploaded |
| Open https://kakurega-eight.vercel.app/try/ and press "Withhold, check, ask" (free tries when switched on; otherwise open "Use my own NEAR AI key") | Two lines of the sample withheld in your browser by the product's own detector; NEAR AI Cloud's hardware proof checked in the tab; an answer; "Signed by the model's own hardware key, and the signature checks out" |

The same capsule from a terminal, after `cargo build -p kk-cli`: `target/debug/kaku verify fixtures/near-live/sample.kkg` prints `VALID`, the signer, the signed claims and the model's signatures.

## 1 minute, nothing to build: ask NEAR yourself

A model key that NEAR's own TDX verifier contract vouched for, read from a public RPC with `curl`. None of our code is involved.

```sh
ARGS=$(printf '{"signer":"89154a43a47684742c3d1753252bb6f49f61da9d8c84fc9b1def2df130cb2d93"}' | base64)
curl -s https://rpc.testnet.fastnear.com -H 'content-type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"query","params":{"request_type":"call_function","finality":"final","account_id":"kakurega-registry-c68af3.testnet","method_name":"hardware","args_base64":"'"$ARGS"'"}}' | python3 -c "import json,sys; print(bytes(json.load(sys.stdin)['result']['result']).decode())"
```

Expected: JSON that starts `{"signer":"89154a43…","tcb_status":"UpToDate","advisory_ids":[],"mr_td":"df4d77c8…`. The transaction that wrote it: [`EqCuP7TZ…`](https://testnet.nearblocks.io/txns/EqCuP7TZmqsgotmj1go7oFg2mzN2qnzRKgmNeXzfhmY). In it our contract passed a fresh NEAR AI Cloud quote for `Qwen/Qwen3.8-27B` to `tee-verifier-2026-07-22.testnet`, and recorded the key only after that contract accepted it.

Then the capsule of a real review that ran on that model, registered for a paid order that required this hardware record:

```sh
ARGS=$(printf '{"capsule_sha256":"2d2f9450a88fab653909a09c3c1a992929e308a0120feb5380a13ea381b99f8b","capsule_signer":"d48949fd672796dd58880341608d6a2fc26e28ade3f323a24792226a324a43ee"}' | base64)
curl -s https://rpc.testnet.fastnear.com -H 'content-type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"query","params":{"request_type":"call_function","finality":"final","account_id":"kakurega-registry-c68af3.testnet","method_name":"get","args_base64":"'"$ARGS"'"}}' | python3 -c "import json,sys; print(bytes(json.load(sys.stdin)['result']['result']).decode())"
```

Expected: `"receipt_count":20`, one signer `89154a43…` for `Qwen/Qwen3.8-27B` with `"receipts":20`, and `"order":"1"`: the contract verified all 20 model signatures in the registration ([`HvDR4pNU…`](https://testnet.nearblocks.io/txns/HvDR4pNUarYwdALySuJR5zNjzxNE7zmxh5KwfXTreSpT)). The capsule itself is `fixtures/near-live/review.kkg`; its SHA-256 is the `capsule_sha256` above.

What this does not show: which software ran (the measurements are recorded, none is approved by the contract), nothing about the GPU, and nothing about whether the review's findings are right.

## 5 minutes: build one program and check four things

Needs git and Rust (the version is pinned in `rust-toolchain.toml`).

```sh
cargo build -p kk-cli
target/debug/kaku chain hardware 89154a43a47684742c3d1753252bb6f49f61da9d8c84fc9b1def2df130cb2d93
target/debug/kaku bench verify benchmarks/leak/results.signed.json --pin "$(cat benchmarks/leak/signer.pub)"
scripts/judge-replay
```

Expected, in order:
- `Recorded at kakurega-registry-c68af3.testnet on NEAR testnet:` then the key, `NEAR's verifier checked its Intel TDX quote against Intel's collateral on 2026-11-13 04:46:56 UTC`, `Intel patch level then: UpToDate`, and a list headed `This does not establish:`.
- `Signature: checks against the pinned key.` then `known shapes: 126 cases. … leaked 0.` and `beyond the detector: 42 cases. … leaked 10.` with each leak named. The leaks are real and listed on purpose: they are passwords and PINs written with no word saying what they are.
- An example capsule the script makes on the spot, with made-up keys and made-up model answers, for the route `example`: accepted, with a list headed `Not established:` whose first line says it is an example. Then the same capsule with one receipt altered: `INVALID: receipt_signature`, then `Both results are as expected.` It prints the path of a one-file page to open from disk; drop either capsule on it. A capsule from a real review is made by `kaku` and checked the same way.

The real NEAR review's capsule is checked offline by `cargo test -p kk-evidence --test near_live`: valid, 20 answers each signed by the key above, and the digest of its receipts equal to the one the registry holds. (It is checked as of a minute after it was made: a capsule expires a day after it is made, so `kaku verify` on that file now reports it expired.)

## 10 minutes: is the contract on NEAR the code in this repository?

Needs `cargo near`.

```sh
scripts/check-deployed
```

Expected: two equal lines `built here : bfeeba19…c3c5` and `on testnet: bfeeba19…c3c5  (kakurega-registry-c68af3.testnet)`, then `MATCH: the account runs exactly this build`. The account has no access keys left, so nobody can replace that code or its fee: `kaku chain fees` shows the fee, 2% on top of the reviewer's amount, sent only to `kakurega-fees-d5de2a.testnet`. The build is not the reproducible Docker one, and the script says so: another machine may produce different bytes from the same source.

## 30 minutes: use it

macOS needs Ollama and Lima; Linux needs bubblewrap. See "Run it" in the [README](../README.md).

```sh
scripts/setup
target/debug/kaku serve          # prints a one-time link; open it
```

Then, in the browser: choose a project folder, name a recipient, **Start review**. When it ends, read **Protection** (what was withheld from the model), **Findings** (only what the kernel reproduced) and **Release** (the exact files and the digest). Approve with `target/debug/kaku approve`, deliver, download the capsule, and check it with `target/debug/kaku verify FILE`. **Try to break it** in the sidebar lets you write your own attack next to two fresh secrets. **Chat**, **Check a confidential model's hardware proof**, with the model name `Qwen/Qwen3.8-27B`, runs the hardware check live against NEAR AI Cloud with no key (about ten seconds).

`tests/e2e/used_flagship.mjs` walks the same path by itself with a real local model and saves screenshots. With your own NEAR AI Cloud account key the same walk runs on NEAR's confidential hardware: `KAKU_TEST_NEAR_MODEL=Qwen/Qwen3.8-27B KAKU_TEST_NEAR_KEY_FILE=FILE node tests/e2e/used_flagship.mjs` (FILE holds a line `NEAR_AI_API_KEY=...` and is readable by you only), or start the app with `kaku serve --route near --near-model Qwen/Qwen3.8-27B --near-key-file FILE`, or type the key in Settings.

## What is not live, said once

- Answers from NEAR AI Cloud were run with the owner's test key; running them again needs a key of your own. Everything above that reads the chain or the fixture needs none.
- NEAR Intents: dry quotes only.
- Everything on chain is testnet. The registry account has no access keys left, so its code and fee cannot be replaced; `scripts/check-deployed` shows the code is this repository's.
- The public site, https://kakurega-eight.vercel.app, is static files: a replay of a real review read from its capsule, and the check page at `/verify/`. Both verify the capsule in your browser. There is no hosted workspace yet, and nothing of ours runs on a server.

## Tracks

| Track | What answers it | Where |
| --- | --- | --- |
| Privacy and Private AI | A kernel outside the model withholds secrets and decides what leaves; hardware evidence for a remote model is checked before use and can be recorded on NEAR by NEAR's verifier; the recipient checks the result offline | `crates/kk-secrets`, `crates/kk-near`, `contracts/kakurega-registry`, `apps/verifier-page` |
| AI Security and Defense | A sandbox with no network for the agent, a broker for every tool call, releases by digest with the owner's approval, a leak benchmark with signed results and named failures | `crates/kk-sandbox`, `crates/kk-broker`, `benchmarks/leak/METHOD.md` |

More: [what is and is not claimed](LIMITS.md), [every feature](FEATURES.md), and one record per component in `docs/engineering-decisions/`.
