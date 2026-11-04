# Kakurega

A confidential AI workspace. The AI works on your private files and code, a security layer outside the model controls what may leave, and the approved result reaches a recipient as a Capsule they can verify themselves.

Status: early development. Nothing here is a finished product yet.

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
