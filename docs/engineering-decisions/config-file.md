# The configuration file (ledger A-A39, second part)

## Reading (by me)
- OpenCode (commit fe04656, MIT): `packages/schema/src/config.ts` in full (145 lines); `packages/core/src/config.ts` lines 117-180 in full and the rest by search (529 lines): PART. NOT READ: `config/managed.ts`, `config/normalize`, the per-section schemas, the TUI tests.
- Codex: `codex-rs/config.md` is a "moved" note in the local collection, so its config code and docs are NOT READ. Hermes' config-file polling (named in matrix A39): NOT READ.

## What OpenCode's config does (VERIFIED in the code above) and where it is weaker for us
- Decoding uses `{ errors: "all", onExcessProperty: "ignore" }`: **every type error is reported at once** (good, kept) but **an unknown key is silently ignored**, so a typo does nothing and nobody is told.
- A malformed or invalid document is logged as a warning and **dropped whole**: the user's settings silently do not apply, with a line in a log as the only sign.
- The config can carry `permissions`, `policies`, `plugins`, `providers`, `mcp`, `share: auto` and `update: auto`: **a config file can loosen security** and switch on automatic installation of updates.
- Text substitution of `{env:...}` and `{file:...}` into the config (a way for a secret to end up in a file or a log): excluded.
- It keeps the last good config when a reload fails (good for a long-running service; ours reads the file when the workspace starts).

## Decisions
- One file, `config.toml`, in the data folder. No layers (no project file, no remote file, no managed file): nothing outside the owner's own folder can change how Kakurega behaves. It is read when the terminal workspace starts, never while it runs.
- The settings are a **closed set that only change how things are shown to the owner**, and each is bound to behaviour that exists today (no decorative keys): `notify` ("auto", "bell", "off": the bell and the desktop notification), `color` ("auto", "never"), `quiet` (start with only the end of a review shown, like `/quiet`), `linear` (start in the plain-line mode for screen readers and pipes). **No key can touch approval, the model route, policy, what leaves the computer, or updates.** A test pins the list and checks it against words that name those things.
- Precedence is command-line flag, then environment variable, then the file, then the default, and `kaku config` shows, for every setting, the value in force and which of these it came from (the idea of Codex's `/debug-config`, NOT READ here, adopted as behaviour only).
- **An unknown key, a wrong type or a value out of range is an error that names the key** (and a nearest known key when there is one), and **all errors are listed at once**. **A file with any error is not applied at all** (a half-applied file is worse than a visible one), the product starts with its defaults and says so at the top of the workspace, in both the full-screen and the plain-line mode, and `kaku doctor` lists each error. It never stops the product starting. (OpenCode: unknown keys ignored, a bad file dropped with only a log line.)
- No substitution, no includes, no secrets: the file holds nothing that needs to stay private, so it is an ordinary file; a file over 64 KiB is refused.
- `kaku config [show|path|check|init]` (`--json` for show and check). `init` writes a commented template with every setting commented out (so it changes nothing) and never overwrites. There is no `set`: rewriting a person's file would lose their comments; editing it by hand is the way, and `check` says whether it is right.
- TOML through the `toml` crate (MIT or Apache-2.0), already in the lockfile as a build dependency; used here only to read, with the validation done by our own closed table so that errors name the key and suggest the nearest one.

## Not built
Keybindings and a command palette (matrix A33: not yet built, so no key for them), reduced motion and timestamps (no such features to bind), live reload while the workspace runs, a per-project file.

## Built
- `crates/kk-cli/src/config.rs`: the closed table `SETTINGS` (notify, color, quiet, linear), `validate` (TOML read by the `toml` crate; every key checked by our own table so each error names the key, with the nearest known key for a likely typo; all errors returned at once), `load` (absent, valid, or invalid; a folder, a file over 64 KiB, bytes that are not text and an unreadable file are each an error, not a crash), `resolve` (a command-line option, then the environment, then the file, then the default, with the source recorded; the environment variables keep the meaning they always had), the template, and `kaku config [show|path|check|init]`. `kk_tui::settings::Settings` carries the display settings to both workspace modes; `Settings::from_env` is the behaviour from before there was a file. `kaku doctor` has a `config` row.
- Dependency: `toml` 0.9 (features `parse` and `serde`). The lockfile gained one edge (`kk-cli` to `toml`) and no new crate; the crate was already there as a build dependency.

## Found by using it, and fixed
- `kaku config --data-dir X show` was refused: the subcommand was the first argument. It is now the first word that is not an option or an option's value (a folder named `show` is a value).
- A stray double parenthesis in the status line, and the PTY scenarios I first wrote had a Python syntax error that kept them from running at all (caught because the suite printed it, then fixed and rerun).
- A parsing check of the position of an error: the column counts characters, not bytes (a stray `b` after two accented letters is column 15, byte 17). Pinned by a test.

## Evidence
- Unit cf01-cf13 (closed set pinned against 26 forbidden words and the defaults; every wrong type and value; all errors at once and none applied; typo suggestions with eight near and far cases; syntax errors with lines and columns including multibyte; the loader's hostile cases; the whole precedence matrix including the environment saying no to `linear` even when the file says yes; the template's own default lines parse to exactly the defaults; init owner-only and never overwriting; the subcommand parser), dc22 (doctor row), tu90-tu91 (the starting verbosity is the same switch as `/quiet`; the notice is one warning card after the welcome in both modes, control characters cleaned).
- Real binary (ee01-ee05): `show --json` with sources from the environment, the file and an option; `check` exit codes and every problem listed; `init`, `path`, a bad command (exit 2), options before the command; the real workspace starting in the plain-line mode from the file alone, the environment overriding it, and a file with one mistake and one correct line applying **none** of it while saying so at the top.
- The real full-screen workspace in a pseudo-terminal (86 of 86 checks): colour present by default, gone with `color = "never"` and with `NO_COLOR`; the "service is not answering" alert is the desktop notification by default, only the bell with `notify = "bell"`, and nothing with `notify = "off"`; a mistake shown on the screen, and the correct line beside it not applied.
- Mutants: 23 in config.rs, 8 in the wiring (kk-tui and kk-cli) and 3 in the full-screen path (34 in all), each caught; two were first missed only because rustfmt had changed the text I targeted, and were rerun against the real text.

## Limits
- It is read when the workspace starts, not while it runs. The settings are only the four that exist; keybindings (matrix A33), reduced motion and timestamps have no feature to bind to yet, so they are not settings. The desktop shell and the web page have their own settings and do not read this file. A real person using a screen reader with `linear = true` has not been tested (NOT VERIFIED).
