# Kakurega

A confidential AI workspace. An AI works on your private files and code, a security layer outside the model decides what may leave, and the approved result reaches a recipient as a capsule they can check themselves.

Status: working prototype on macOS and Linux. Not a finished product: see [docs/LIMITS.md](docs/LIMITS.md) before relying on it.

## What it does today

- Review a project privately: an agent reads it inside a sealed sandbox, secrets are withheld from it, and the kernel re-runs each finding's reproducer on a minimal extract. Only confirmed findings can be released.
- Release only what you approve: you see the exact files, the cumulative disclosure to that recipient and what the release does not establish, then approve in the desktop window (Touch ID prompt), the terminal workspace or `kaku approve`.
- A signed capsule the recipient verifies offline, in a browser at `/verify` or with `kaku verify`, with a second independent verifier in JavaScript.
- Private memory and chat on your computer: notes with privacy labels, a reason and a history; forgetting deletes the text for good; chat uses notes as data and keeps instruction-like notes away from the model.
- A signed, hash-chained activity log you can verify anywhere (`kaku log verify`).
- Surfaces: browser workspace (English and Hindi), full-screen terminal workspace (`kaku`), desktop app (Electron), MCP server (`kaku mcp`, counts only), a minimum-permission browser extension, a read-only Telegram bridge.
- Models: a local model through Ollama. NEAR AI Cloud model attestation is verified for real; NEAR completions, encrypted inference and signed responses need an API key and are not wired yet.

## Run it (macOS)

```
scripts/setup                        # dependencies, web build
ollama serve &                       # and: ollama pull qwen2.5:3b
limactl start kak                    # Linux VM with bubblewrap and Rust (the sandbox on macOS)
cargo build -p kk-cli
target/debug/kaku                    # full-screen workspace; starts the service if needed
target/debug/kaku serve              # or the browser workspace: prints a one-time pairing link
```

Desktop: `cd apps/desktop && pnpm install && pnpm start` with `KAKU_BIN` pointing at the built `kaku`.

## Check it

```
scripts/test                         # build, lints, unit and integration tests, layer rule, secret scan
scripts/e2e                          # a real browser drives a full review, approval, memory, chat and log
scripts/test-lima                    # the sandbox and worker tests inside the Linux VM
python3 scripts/verify_program.py I  # the acceptance gate for a milestone (also II and III)
```

`program/ACCEPTANCE_MANIFEST.json` lists every acceptance row with the command that proves it; rows that need a person or an external key say so. `TESTING.md`, `PROGRESS.md` and `HUMAN.md` (one level up in the working tree) record evidence, state and what only the owner can do.

## Layout

- `crates/` Rust crates (trusted core). The layer rule in `scripts/layers.json` is checked by `scripts/check_layers.py`.
- `apps/` web workspace, desktop app, browser extension.
- `packages/` shared TypeScript packages (canonical JSON, capsule verifier).
- `fixtures/`, `tests/` test inputs and cross-component tests.
- `docs/engineering-decisions/` one record per component: target experience, what was read, decision, evidence, what is not covered.
- `docs/mcp.md` the MCP server. `docs/LIMITS.md` what is and is not claimed.
- `program/` acceptance manifest and the parity registry of every planned capability.

## Requirements

git, Rust (version pinned in `rust-toolchain.toml`), Node 22.19 or newer, pnpm, Python 3. macOS also needs Lima for the sandbox; Linux needs bubblewrap. Windows is not supported.

## Licence

The licence of Kakurega itself has not been decided, so the repository is private. Third-party credits are in `NOTICE.md`.
