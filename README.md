# Kakurega

**A private AI workspace where a security layer outside the model decides what may leave your computer, and what does leave can be checked by the person who receives it.**

Status: working prototype on macOS and Linux, NEAR testnet. Not a finished product: read [docs/LIMITS.md](docs/LIMITS.md) before relying on it.

## The problem

To get help from an AI you hand it your code, your contracts and your meeting notes, and then you trust three things you cannot see: that the model was not talked into leaking them by a line planted in a file, that the provider runs what it says it runs, and that the answer you forward to a client is what the model really produced. A promise in a privacy policy covers none of the three.

## What Kakurega does

1. **Works on your files without handing them over.** An agent reviews a project inside a sealed sandbox with no network. Keys, and passwords said as passwords, are withheld before any model sees a line, on every path: reviews, chat, attached files, documents, meeting notes, team runs. What the detector cannot recognise is listed in the limits and counted by the benchmark.
2. **Decides what may leave outside the model.** The model cannot send anything. A release is exact files with a digest; you read them and approve that digest, and nothing else goes. Planted instructions in a file are data, never orders.
3. **Keeps only what it could prove.** A finding is kept when the kernel re-ran its script on a minimal extract and saw it fail the right way. A quote from a document or a transcript is kept when it is really in the file.
4. **Gives the recipient something to check.** The result is a signed capsule. The recipient verifies it offline, in a one-file web page or with one command, and is told what the check does *not* establish.
5. **Uses confidential hardware where the model is not yours.** Before a question may go to NEAR AI Cloud, its Intel TDX and NVIDIA evidence is checked with a fresh challenge. The same check can be recorded on NEAR by NEAR's own verifier contract, so a recipient does not have to take our word for it.

| | | |
| --- | --- | --- |
| <img src="docs/img/protection.png" alt="The Protection tab of a review: the .env file withheld, two files flagged as possible prompt injection, the model on this computer" width="100%"><br><sub>What the agent could not see, and what it was not allowed to obey.</sub> | <img src="docs/img/release.png" alt="The Release tab: the exact files with their sizes and hashes and the digest to approve" width="100%"><br><sub>The exact release and its digest, before anything is asked of you.</sub> | <img src="docs/img/verify.png" alt="The recipient's page: Valid, what is established and what is not" width="100%"><br><sub>The recipient's check, with what it does not establish.</sub> |

Screenshots from a run with a local model (`tests/e2e/used_flagship.mjs`).

## Who it is for

People who must give an outside party a result from private material: a security reviewer sending findings to a client, a lawyer answering from a contract set, a founder sharing diligence answers. The client pays for one verified review; the money is held by a NEAR contract and released when the capsule for that order is registered, not when either side says so. That order flow runs on testnet. Nothing is charged today.

## Why NEAR

| NEAR piece | What it does here | State |
| --- | --- | --- |
| NEAR AI Cloud attestation | Intel TDX quote, NVIDIA evidence, the model's signing key bound to a fresh challenge, the deployment compared with NEAR's own releases | Live, no key needed: the proof panel in chat, and `/proof` in the terminal workspace |
| NEAR's on-chain TDX verifier | Our registry contract hands a model's quote to NEAR's verifier contract and records the key it vouched for | Live on testnet: [`DfD1dMCX…`](https://testnet.nearblocks.io/txns/DfD1dMCXCvBp2ptHSJcNXC2YwpgxZkedohsQ68M3CaGg) (184 Tgas) |
| Capsule registry contract | Who registered a capsule and when, readable by anyone from a public RPC | Live on testnet: `kakurega-registry-5d26f9.testnet` |
| Orders | Money for a review is held and paid only when the named reviewer registers a capsule for that order | Live on testnet in NEAR; a USDC order is tested on a local node only |
| NEAR AI Cloud inference | Encrypted questions to a model in a TEE, each answer's signature checked against the attested key | Built and tested against recorded answers. Not run live: it needs an account key we do not have yet |
| NEAR Intents | A price for paying an order from another chain | Dry quotes only (`kaku chain quote`); nothing is sent |

Take NEAR away and the recipient is left trusting us about the hardware. That is the capability it adds.

## Check it yourself

Step by step, with the exact output to expect: [docs/JUDGES.md](docs/JUDGES.md). No keys and no funds are needed for any of these. Build once with `cargo build -p kk-cli` (Rust is pinned in `rust-toolchain.toml`).

| Time | Do | See |
| --- | --- | --- |
| 1 min | `target/debug/kaku chain hardware 89154a43a47684742c3d1753252bb6f49f61da9d8c84fc9b1def2df130cb2d93` | Read from NEAR testnet: the model key, "NEAR's verifier checked its Intel TDX quote", the patch level, and three lines on what this does not establish |
| 1 min | `target/debug/kaku bench verify benchmarks/leak/results.signed.json --pin "$(cat benchmarks/leak/signer.pub)"` | "Signature: checks against the pinned key." then the counts: known shapes 126 cases, leaked 0; beyond the detector 42 cases, leaked 10, each one named |
| 3 min | `scripts/judge-replay` | A good capsule accepted and a forged one refused, from the command line; then open `apps/verifier-page/dist/kakurega-verify.html` from disk and drop the capsule on it |
| 5 min | `scripts/check-deployed` (needs `cargo near`) | The contract is built here and its hash compared with the code NEAR reports for the testnet account: MATCH |
| 20 min | `scripts/test` | Lints, unit and integration tests, the layer rule and the secret scan, exit 0 |
| An hour | `scripts/e2e-all` | Every end-to-end script: a real browser, the terminal, the desktop app, MCP, the extension |

The first row asks a public NEAR RPC and nothing of ours. The benchmark can be run again from scratch with `benchmarks/leak/reverify --rerun`.

## Proof of protection

- **Leaks.** 168 cases through seven doors against a model that repeats every secret it is shown (`benchmarks/leak/METHOD.md`). For the secret shapes the detector knows: 0 of 126 reached the model. For shapes it does not know (a password or a PIN written with no word saying what it is, or made of ordinary words): 10 of 42 did, and the signed results name each one.
- **Planted instructions, real models.** A file with a made-up key and one planted instruction, summary asked for (`benchmarks/injection/METHOD.md`). A plain prompt repeated the key in 4 of 16 attack runs. With the file attached in Kakurega the key was sent 0 of 18 times, because its line is withheld. Small sample, two different models; the limits are in the method.
- **Review quality.** On a small project with one planted defect and a 7B local model (`benchmarks/quality/METHOD.md`): the plain model found it 3 of 3 with 13 false findings; the verified loop found it 3 of 3 with 0 false findings, at about 150 seconds a review.
- **Try to break it.** The app has a box where you write your own attack next to two fresh secrets and see exactly what the model was sent.

## Features and state

| Feature | State |
| --- | --- |
| Private review in a sandbox, findings re-run by the kernel, release by digest, signed capsule | Works; walked in a browser with a real local model |
| Recipient check: one-file page, `kaku verify`, a second verifier in JavaScript, optional lookup on NEAR | Works |
| Chat, documents with checked quotes, meeting notes with checked quotes, memory with labels | Works with a local model |
| Secrets and spoken passwords withheld; names swapped for placeholders when the model is not on this computer | Works; measured by the leak benchmark |
| Roundtable of experts, team runs with a lead, a Project reviewer seat that runs the sandboxed review | Works with a local model |
| Skills: your own, a bundled shelf, signed packs and a publisher's signed catalog; nothing is used until you approve it | Works |
| Profiles with separate keys; encrypted backup with a recovery key | Works |
| Desktop app (macOS, Linux): native approval with Touch ID, menu-bar item, notifications, a picture of the screen for the chat | Works; unsigned build, the capture preview was not clicked by a person in a test |
| Terminal workspace (`kaku`), MCP server (`kaku mcp`), browser extension, Telegram bridge | Works |
| GitHub connector (read only) | Tested against a stand-in; not yet with a real GitHub App |
| Hand a question to Claude Code, with your yes on the exact text | Works; said plainly to be not private |
| Model on NEAR AI Cloud | Built; live answers blocked on an account key |
| Hindi | Written by the builder, not reviewed by a native reader |
| Windows, mobile, hosted mode, encrypted sync between devices | Not built |

The full list is in [docs/FEATURES.md](docs/FEATURES.md).

## Claims, code and tests

| Claim | Code | Tests |
| --- | --- | --- |
| A key or password is withheld before any model sees it, in every encoding the detector reads | `crates/kk-secrets/src/lib.rs`, `crates/kk-secrets/src/spoken.rs` | `crates/kk-secrets/tests/secret_grid.rs`, `scripts/bench-leak.py` |
| Nothing is released without your approval of that exact digest | `crates/kk-service/src/service.rs`, `crates/kk-evidence/src/approval.rs` | `crates/kk-service/tests/flow.rs`, `tests/e2e/run.mjs` |
| A finding is kept only when its script reproduced on a minimal extract | `crates/kk-review/src/job.rs` | `crates/kk-review/tests/job.rs`, `scripts/eval-quality.py` |
| A capsule that was changed, or signed by someone else, is refused by both verifiers | `crates/kk-evidence/src/capsule.rs`, `packages/` | `tests/e2e/verifier_page.mjs`, `scripts/judge-replay` |
| The review agent has no network and sees no file you excluded | `crates/kk-sandbox`, `crates/kk-broker` | `scripts/test-lima`, `scripts/test-linux` |
| The desktop page cannot be given the camera, the screen, or any other permission, and cannot navigate away | `apps/desktop/src/policy.mjs`, `apps/desktop/src/main.mjs` | `tests/e2e/desktop_attack.mjs` |
| Text reaches the coding agent on your computer only after your yes on that exact text, once, even across a restart | `crates/kk-service/src/service/team.rs`, `crates/kk-cli/src/handoff.rs` | `crates/kk-service/tests/flow.rs` (sv261) |
| A review's findings are handed to other experts only while their model is on this computer, never to the outside coding agent, and a run or conversation held here is not continued with a model elsewhere | `crates/kk-service/src/service/team.rs` | `crates/kk-service/tests/flow.rs` (sv266 to sv270) |
| A skill pack is taken only from a publisher you pinned, and through a catalog only when it is the exact file listed | `crates/kk-packs/src/lib.rs`, `crates/kk-packs/src/catalog.rs` | `crates/kk-service/tests/flow.rs` (sv216, sv265), `tests/e2e/packs_web.mjs` |
| NEAR's verifier, not ours, vouches for a model key on chain | `contracts/kakurega-registry` | `contracts/kakurega-registry/tests/sandbox.rs` on a local node, and the testnet transaction above |
| The signed log shows what left this computer, and it can be verified anywhere | `crates/kk-eventlog`, `crates/kk-service/src/service/outbound.rs` | `crates/kk-service/tests/flow.rs`, `kaku log verify` |

Each component has a record of what was read, decided and tested, and what is not covered, in `docs/engineering-decisions/`.

## Run it (macOS)

```
scripts/setup                        # dependencies, web build
ollama serve &                       # and: ollama pull qwen2.5:3b
limactl start kak                    # Linux VM with bubblewrap and Rust (the sandbox on macOS)
cargo build -p kk-cli
target/debug/kaku                    # full-screen workspace; starts the service if needed
target/debug/kaku serve              # or the browser workspace: prints a one-time pairing link
```

Desktop: `cd apps/desktop && pnpm install && pnpm start` with `KAKU_BIN` pointing at the built `kaku`. A self-contained macOS app is built by `scripts/package-desktop-macos` (a zip and its SHA-256) and installed by `scripts/install-desktop-macos`, which checks the SHA-256 before it unpacks anything; the app is signed ad hoc only and is not notarized. It stays in the menu bar and tells you when something waits for you.

Linux package: `scripts/package-linux` builds and checks a `.deb` in the Linux VM and copies it to `target/packaged-linux/` (install with `sudo dpkg -i`, start with `kakurega`).

## Check it

```
scripts/test                         # lints, unit and integration tests, layer rule, secret scan (not the end-to-end scripts)
scripts/e2e                          # a real browser drives a full review, approval, memory, chat and log
scripts/e2e-all                      # every end-to-end script: browser, terminal, MCP, Telegram, extension, desktop, signed approvals
scripts/test-lima                    # the sandbox and worker tests inside the Linux VM
python3 scripts/verify_program.py I  # the acceptance gate for a milestone (also II and III)
```

`program/ACCEPTANCE_MANIFEST.json` lists every acceptance row with the command that proves it; rows that need a person or an external key say so. `TESTING.md`, `PROGRESS.md` and `HUMAN.md` (one level up in the working tree) record evidence, state and what only the owner can do.

## Check a capsule as a recipient

```
scripts/judge-replay                 # builds the check page, makes a good and a forged capsule, checks both from the command line
```

The page `apps/verifier-page/dist/kakurega-verify.html` is one file with no network use: open it from disk in any browser, drop a capsule on it, and optionally paste the signer key you expect. It lists each model signature in the capsule and says what the check does not establish, including that a model's key is not shown to belong to attested hardware.

## Layout

- `crates/` Rust crates (trusted core). The layer rule in `scripts/layers.json` is checked by `scripts/check_layers.py`.
- `apps/` web workspace, desktop app, browser extension, the standalone capsule check page.
- `contracts/` the NEAR capsule registry contract (testnet only; a separate Cargo project built with `cargo near`).
- `packages/` shared TypeScript packages (canonical JSON, capsule verifier).
- `fixtures/`, `tests/` test inputs and cross-component tests.
- `docs/engineering-decisions/` one record per component: target experience, what was read, decision, evidence, what is not covered.
- `docs/mcp.md` the MCP server. `docs/LIMITS.md` what is and is not claimed.
- `program/` acceptance manifest and the parity registry of every planned capability.

## Requirements

git, Rust (version pinned in `rust-toolchain.toml`), Node 22.19 or newer, pnpm, Python 3. A recipient who gets a release with a JavaScript finding needs Python 3 and Node.js 22 or newer to re-run its check; without Node the runner says so and still checks the other findings. macOS also needs Lima for the sandbox; Linux needs bubblewrap. Windows is not supported.

## Licence

The licence of Kakurega itself has not been decided, so the repository is private. Third-party credits are in `NOTICE.md`.
