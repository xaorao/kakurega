# How Kakurega is tested

Every claim in the README and in `docs/LIMITS.md` is meant to be checked by a command in this repository. This page says
which suite checks what, what runs only by hand and why, and where the evidence lives.

## The suites

| Command | What it runs |
|---|---|
| `scripts/fast-check` | Formatting, lints and the tests of the crates and web code that changed, the secret scan and the layer and error-text rules. Run before every commit. |
| `scripts/test` | The secret scan, the layer, error-text and log-label rules, formatting and lints, every Rust test in the workspace, the documentation claims, and every JavaScript package's tests (the web app, and the recipient's checker held to the shared capsule and share vectors in `fixtures/`). |
| `scripts/e2e` | A real browser drives the main journey: a review, the approval, delivery, the capsule check, memory, chat and the signed log. |
| `scripts/e2e-all` | Every scripted end-to-end test: browser screens, the full-screen terminal through a real terminal, MCP, the Telegram bridge against a stand-in for Telegram's API, the browser extension, the desktop app (crashes, restarts, a service already running, packaging), signed approvals, a real full disk, the recipient's check page in browsers with and without Ed25519. |
| `scripts/test-lima` | From the Mac, the transport to the Linux virtual machine reviews run in, and the review checks, Rust checks and coding tasks run through it in the real sandbox. |
| `scripts/test-linux` | Inside that Linux machine, the sandbox, run and review crates' tests with the review worker built there. |
| `python3 scripts/verify_program.py I` | The acceptance gate of a milestone (also `II` and `III`): each row of `program/ACCEPTANCE_MANIFEST.json` with the command that proves it. |

A change is pushed only after `scripts/test`, `scripts/e2e-all`, `scripts/test-lima` and `scripts/test-linux` pass on the revision that is pushed.

## Run by hand, and why

These need something a scripted run cannot assume; each says at its top what it needs.

- **A real local model** (Ollama with a model installed): `tests/e2e/used_flagship.mjs`, `used_workspace.mjs`, `used_terminal.py`, `code_real.mjs`, `doc_index.mjs`, `methods_web.mjs`, `mcp_ask_web.mjs`, `notes_only.mjs`. They judge what a person gets back, so they use the model a person would.
- **A model elsewhere**: NEAR AI Cloud (`KAKU_TEST_NEAR_MODEL=Qwen/Qwen3.8-27B KAKU_TEST_NEAR_KEY_FILE=FILE`, where FILE holds `NEAR_AI_API_KEY=...` and only you can read it) runs `used_flagship.mjs`, `used_workspace.mjs`, `used_terminal.py`, `code_real.mjs`, `mcp_ask_web.mjs`, `hide_web.mjs` and `route_web.mjs` (the key typed in Settings) on its confidential hardware; `roundtable_web.mjs` and `team_web.mjs` take `near:MODEL` as their argument; `scripts/eval-quality.py near:MODEL 3` and `scripts/bench-injection-chat.py 2 --near MODEL` rerun the benchmarks. The outside test route (`ANTHROPIC_API_KEY`) still serves `hide_web.mjs` and the part of `tests/e2e/full_disk.py` that checks nothing leaves unless the signed log recorded it.
- **NEAR**: `cargo test -p kk-near -- --ignored` checks a live model attestation from NEAR AI Cloud (no account needed); `KAKU_LIVE_CHAIN=1 node tests/e2e/verifier_page.mjs` reads a real registration from NEAR testnet; the testnet part of `tests/e2e/chain_register.py` needs a throwaway testnet account (`KAKU_TESTNET_KEY_FILE`, `KAKU_TESTNET_ACCOUNT`).
- **Claude Code**: `KK_HANDOFF_REAL=1` lets the browser test hand one line to the real Claude Code.
- **Every screen in both themes and languages, desktop and phone**: `node tests/e2e/audit_matrix.mjs` takes the screenshots for a person to look at.

## Where the evidence is

- `program/ACCEPTANCE_MANIFEST.json`: every acceptance row and the command that proves it; rows that need a person or an external key say so.
- `benchmarks/leak/`: the leak benchmark's method, its signed results and the key to check them with (`kaku bench verify`).
- `fixtures/capsule/` and `fixtures/share/`: the vectors both checkers (Rust and JavaScript) are held to.
- `fixtures/near-live/review.kkg`: the capsule of a real review on NEAR AI Cloud, registered on testnet; `cargo test -p kk-evidence --test near_live` checks it offline against the values the registry holds.
- `docs/engineering-decisions/`: per component, what was studied, what was decided and how it was checked.
- `docs/LIMITS.md`: what is not claimed, and what has not been verified.
