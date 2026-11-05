# Reproducers beyond Python

## Status (honest)
Steps 1 and 2 are built and tested (a `language` field on a finding; JavaScript closure and runner, mutation-checked). **They were built before any reference was read, which breaks the source-first rule: READING OWED.** No claim of "best" or "comparable to the strongest tools" is made for this feature until the references named under "References and what they teach" are read in full along their execution paths and this record is updated with what they showed. Steps 3 to 5 do not start until then.

## Why this matters

A private code review is only as useful as the languages it can check. Today every finding must come with a **Python**
reproducer, and every fix is verified by re-running it (`crates/kk-review/src/check.rs`). A JavaScript, TypeScript or Rust
project can be read by the model, but none of its findings can be confirmed or its fixes verified. Most real projects the
owner will bring are not Python. This is the largest limit on the flagship workflow.

## What exists today (VERIFIED by reading the code)

- `job.rs` `SYSTEM_PROMPT` tells the model to report a finding with "a Python reproducer" that prints `CONTROL_OK`, then
  `REPRODUCED` only if the defect is present.
- `check.rs` `parse_finding` accepts exactly `title, file, line, explanation, reproducer` (anything else is refused).
- `python_closure` finds the cited file's Python imports (`import x`, `from x import y`) among the project files, refuses a
  closure that needs a withheld (Restricted) file, and copies only those files into a scratch sample.
- `run_reproducer` runs `python3 -B repro.py` three times in the sandbox (no network, read-only binds, 30 s wall, 15 s CPU):
  twice to compare outputs (non-determinism is a verdict) and once with the cited file emptied (the negative control: the
  script must stop printing `REPRODUCED`). It requires a `CONTROL_OK` line.
- Fix verification (`check.rs`, `FixStatus`) re-runs the same reproducer on the changed file; the project test oracle runs
  Python `unittest` only (`scripts/test-lima`, `II-09j`).
- The release capsule ships `run-check.py` (`release.rs`, `VERIFY_SCRIPT`) so the recipient can rebuild the sample and
  re-run `repro-N.py`.

## Target experience

An owner points Kakurega at a JavaScript or TypeScript project. The model reports a finding in the project's own language;
Kakurega confirms it with the same three-run, negative-control rules; a proposed fix is verified the same way; the recipient
of the release can re-run the check with the language's own interpreter. The words "reproduced", "not reproduced",
"nondeterministic", "unavailable" mean the same in every language, and the screens say which language a check used.

## References and what they teach

Not yet studied for this feature. Owed before building: how Semgrep, CodeQL and agent harnesses (OpenHands, Codex, Cline)
decide that a finding is real, which languages their sandboxes cover, and how they run project tests. Kakurega's
reproducer-with-negative-control is a stricter evidence rule than most; the goal is to keep that rule, not to weaken it for
convenience. Recording this gap here so it is not mistaken for done.

## Chosen approach

1. **A `language` field on a finding**, an enumerated value (`python`, `javascript`), default `python` so existing findings
   and reports are unchanged. `parse_finding` stays strict: an unknown language is refused, never guessed.
2. **One interpreter table, not scattered `python` strings.** Each language names its interpreter path, script name, the
   arguments, the environment, the module-path rules and the extensions it treats as source. `CheckEnv` carries the paths
   (`python` today; `node` added).
3. **JavaScript closure**: static `require('./x')`, `import ... from './x.js'`, `import('./x.js')` with literal relative
   specifiers, resolved with the extensions `.js .mjs .cjs` and `/index.js`. Bare package names are not followed (no
   `node_modules`); a reproducer that needs a third-party package is `unavailable`, with the package named. A closure that
   needs a Restricted file is refused exactly as for Python.
4. **Runner**: `node` inside the same sandbox spec. Defense in depth: Node's own permission flags (read only the sample and
   the script) are added if the VM's Node supports them; the sandbox remains the real boundary.
5. **TypeScript is a separate, later step** (Node can strip types, with limits: no enums or namespaces). Not promised here.
6. **Release**: a `run-check.mjs` beside `run-check.py`, chosen per finding; the release README names which interpreter the
   recipient needs. The signed release digest covers both scripts.
7. **Test oracle**: `node --test` for projects that have tests, run before and after a fix, like `unittest` today. A project
   whose tests cannot run offline is reported as "tests not run", never as passing.
8. **Readiness**: a setup check that says whether the sandbox has `node` (the Linux VM has Node 22 at `/usr/bin/node`; a
   direct Linux install needs it installed).

## Acceptance (written before the code)

Each is a test that must fail on a build without the feature:

- A fixture JavaScript project with a real defect: reproduced, control passes, empty-file control no longer reproduces.
- The same reproducer made non-deterministic is reported nondeterministic; one that prints `REPRODUCED` without
  `CONTROL_OK` is failed.
- Hostile reproducers: network, file write, `child_process`, `worker_threads`, reading outside the sample, an infinite loop,
  a huge output, a closure that reaches a withheld file. Each ends in a refusal or a limit, never a pass.
- A cosmetic, disabling, broken and ambiguous fix is refused exactly as for Python (the `f0x` tests, in JavaScript).
- The recipient's `run-check.mjs` reproduces the verdict on a machine with Node and says clearly when Node is missing.
- A mixed project (Python and JavaScript findings) keeps each finding's language; Python behaviour is byte-for-byte
  unchanged (all existing `check`, `job`, `release` tests pass untouched).
- The model prompt change is checked against the real local model on a fixture: whether it picks the right language and
  writes a reproducer that passes `CONTROL_OK` is measured and the rate recorded, not assumed.

## Unknowns

- Whether a small local model writes good JavaScript reproducers at all (measure on the real model; if the rate is poor,
  say so and keep the feature behind the model's measured ability).
- Node's permission flags on the VM's Node build.
- Direct-Linux installs without Node.

## Build order

1. Language table and `language` field, Python through the new path with no behaviour change.
2. JavaScript closure and runner with the hostile-reproducer tests.
3. Fix verification and the Node test oracle.
4. Release scripts and recipient re-run.
5. Prompt, screens (language shown), readiness check, live model measurement.

Each step passes the full suites before the next starts.
