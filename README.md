# Kakurega

A confidential AI workspace. The AI works on your private files and code, a security layer outside the model controls what may leave, and the approved result reaches a recipient as a Capsule they can verify themselves.

Status: early development. Nothing here is a finished product yet.

## What works today

- `kaku serve` runs a local workspace in your browser: pair, choose a project folder, an agent reviews it inside a sealed sandbox, the kernel re-runs each finding's reproducer on a minimal extract, you approve the exact files in a terminal, and a signed capsule is delivered.
- `kaku verify` and the browser page at `/verify` check a capsule offline and say what it does not establish.
- Models: a local model through Ollama (private, runs on this computer). NEAR AI Cloud attestation is verified for real; NEAR completions need an API key and are not wired yet.
- Sandbox: Linux bubblewrap, used directly on Linux and through a Linux virtual machine (Lima) on macOS.

## Run it (macOS)

```
scripts/setup
ollama serve &                       # and: ollama pull qwen2.5:1.5b
limactl start kak                    # Linux VM with bubblewrap and Rust
scripts/e2e                          # builds the worker in the VM, the CLI and the web app, then drives a real browser
target/debug/kaku serve              # prints a one-time pairing link
```

Honest limits: small local models find few defects; the review results of real models are not measured yet. The sandbox on macOS needs the virtual machine. Windows is not supported.

## Develop

```
scripts/setup
scripts/test
```

Requires git, Rust (version pinned in `rust-toolchain.toml`), Node 22.19 or newer, pnpm and Python 3.

## Layout

- `crates/` Rust crates (trusted core). The layer rule in `scripts/layers.json` is checked by `scripts/check_layers.py`.
- `apps/` user-facing applications.
- `packages/` shared TypeScript packages.
- `fixtures/`, `benchmarks/`, `tests/` test inputs, measurements and cross-component tests.
- `docs/engineering-decisions/` one record per component.
- `program/` progress and acceptance records.

## Licence

The licence of Kakurega itself has not been decided. Third-party credits are in `NOTICE.md`.
