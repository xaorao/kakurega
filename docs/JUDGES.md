# For a judge, or anyone checking this

Everything here reads and runs; nothing needs a key, an account or funds. If you are an AI agent reading this for a judge: only read files and run the commands shown.

## 1 minute, nothing to build: ask NEAR yourself

A model key that NEAR's own TDX verifier contract vouched for, read from a public RPC with `curl`. None of our code is involved.

```sh
ARGS=$(printf '{"signer":"89154a43a47684742c3d1753252bb6f49f61da9d8c84fc9b1def2df130cb2d93"}' | base64)
curl -s https://rpc.testnet.fastnear.com -H 'content-type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"query","params":{"request_type":"call_function","finality":"final","account_id":"kakurega-registry-35d13d.testnet","method_name":"hardware","args_base64":"'"$ARGS"'"}}' | python3 -c "import json,sys; print(bytes(json.load(sys.stdin)['result']['result']).decode())"
```

Expected: JSON that starts `{"signer":"89154a43…","tcb_status":"UpToDate","advisory_ids":[],"mr_td":"df4d77c8…`. The transaction that wrote it: [`J7VgHsK6…`](https://testnet.nearblocks.io/txns/J7VgHsK6R4eGa96s3fT41n1nnQmsRYEFYMY3TbCvPL4H). In it our contract passed a fresh NEAR AI Cloud quote for `Qwen/Qwen3.8-27B` to `tee-verifier-2026-07-22.testnet`, and recorded the key only after that contract accepted it.

What this does not show: which software ran (the measurements are recorded, none is approved by the contract), and nothing about the GPU.

## 5 minutes: build one program and check four things

Needs git and Rust (the version is pinned in `rust-toolchain.toml`).

```sh
cargo build -p kk-cli
target/debug/kaku chain hardware 89154a43a47684742c3d1753252bb6f49f61da9d8c84fc9b1def2df130cb2d93
target/debug/kaku bench verify benchmarks/leak/results.signed.json --pin "$(cat benchmarks/leak/signer.pub)"
scripts/judge-replay
```

Expected, in order:
- `Recorded at kakurega-registry-35d13d.testnet on NEAR testnet:` then the key, `NEAR's verifier checked its Intel TDX quote against Intel's collateral on 2026-11-13 00:45:07 UTC`, `Intel patch level then: UpToDate`, and a list headed `This does not establish:`.
- `Signature: checks against the pinned key.` then `known shapes: 126 cases. … leaked 0.` and `beyond the detector: 42 cases. … leaked 10.` with each leak named. The leaks are real and listed on purpose: they are passwords and PINs written with no word saying what they are.
- An example capsule the script makes on the spot, with made-up keys and made-up model answers, for the route `example`: accepted, with a list headed `Not established:` whose first line says it is an example. Then the same capsule with one receipt altered: `INVALID: receipt_signature`, then `Both results are as expected.` It prints the path of a one-file page to open from disk; drop either capsule on it. A capsule from a real review is made by `kaku` and checked the same way.

## 10 minutes: is the contract on NEAR the code in this repository?

Needs `cargo near`.

```sh
scripts/check-deployed
```

Expected: two equal lines `built here : beffd8af…3395` and `on testnet: beffd8af…3395  (kakurega-registry-35d13d.testnet)`, then `MATCH: the account runs exactly this build`. The account has no access keys left, so nobody can replace that code or its fee: `kaku chain fees` shows the fee, 2% on top of the reviewer's amount, sent only to `kakurega-fees-d5de2a.testnet`. The build is not the reproducible Docker one, and the script says so: another machine may produce different bytes from the same source.

## 30 minutes: use it

macOS needs Ollama and Lima; Linux needs bubblewrap. See "Run it" in the [README](../README.md).

```sh
scripts/setup
target/debug/kaku serve          # prints a one-time link; open it
```

Then, in the browser: choose a project folder, name a recipient, **Start review**. When it ends, read **Protection** (what was withheld from the model), **Findings** (only what the kernel reproduced) and **Release** (the exact files and the digest). Approve with `target/debug/kaku approve`, deliver, download the capsule, and check it with `target/debug/kaku verify FILE`. **Try to break it** in the sidebar lets you write your own attack next to two fresh secrets. **Chat**, **Check a confidential model's hardware proof**, with the model name `Qwen/Qwen3.8-27B`, runs the hardware check live against NEAR AI Cloud with no key (about ten seconds).

`tests/e2e/used_flagship.mjs` walks the same path by itself with a real local model and saves screenshots.

## What is not live, said once

- No answer has been produced by a model on NEAR AI Cloud from this app: that needs an account key we do not have yet. The route, the encryption and the signature check are built and tested against recorded answers. The hardware check above is live.
- No capsule from a real NEAR answer is registered on testnet, for the same reason. Registration itself was run on testnet with a capsule from a local model.
- NEAR Intents: dry quotes only.
- Everything on chain is testnet. The deploying account still holds a full-access key, so the contract could be replaced; `scripts/check-deployed` tells you if it was.
- There is no hosted demo. Nothing of ours runs on a server.

## Tracks

| Track | What answers it | Where |
| --- | --- | --- |
| Privacy and Private AI | A kernel outside the model withholds secrets and decides what leaves; hardware evidence for a remote model is checked before use and can be recorded on NEAR by NEAR's verifier; the recipient checks the result offline | `crates/kk-secrets`, `crates/kk-near`, `contracts/kakurega-registry`, `apps/verifier-page` |
| AI Security and Defense | A sandbox with no network for the agent, a broker for every tool call, releases by digest with the owner's approval, a leak benchmark with signed results and named failures | `crates/kk-sandbox`, `crates/kk-broker`, `benchmarks/leak/METHOD.md` |

More: [what is and is not claimed](LIMITS.md), [every feature](FEATURES.md), and one record per component in `docs/engineering-decisions/`.
