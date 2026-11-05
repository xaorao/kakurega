# Reproducers beyond Python

## Status (honest)
Reading is done (see "References read"). Built and tested: a `language` field on a finding; the JavaScript closure and runner; and, from the reading, three corrections to fix checking that apply to both languages (a fix may not change a test file, the project's tests must still all run, and a missing package is "cannot check here", not a failed reproducer). Still to build: the JavaScript test oracle, the release scripts for JavaScript, the prompt and screens, the readiness check, and the measurement on the real local model. No claim of "best" or "comparable to the strongest tools" is made; the comparison below says where ours is stricter and where it is not.

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

## References read (by me)
- **SWE-bench** (`SWE-bench/SWE-bench` commit 02e7a74, MIT): `swebench/harness/grading.py` in full (393 lines); `infra_failure.py` in full; `log_parsers/__init__.py`, `rust.py`, `go.py` and the TAP parser of `javascript.py`; `run_evaluation.py` lines 296-432 and `utils.py` 200-250 (patch apply, test run, timeout, exit-code capture). NOT READ: the image builder, the 40 other per-project parsers, the Modal path.
- **Agentless** (`OpenAutoCoder/Agentless` commit 5ce5888, MIT): `agentless/test/generate_reproduction_tests.py` lines 1-260 (the prompt that asks for "Issue reproduced / resolved / Other issues"), `run_reproduction_tests.py` in full, `run_tests.py` lines 50-80, 165-210, 440-488 (how a reproduction is judged). NOT READ: the repair and localisation stages, test selection.
- **Semgrep** (`semgrep/semgrep` commit 692bfae, LGPL-2.1, ideas only): `cli/src/semgrep/test.py` lines 55-270, 444-470, 590-670 (expected-versus-reported line sets, autofix compared with an expected file). NOT READ: the engine, the language registry, taint mode.
- **Codex** (`openai/codex` commit f2b94ef, Apache-2.0): the sandbox, see `linux-sandbox.md` (seccomp.rs in full, bwrap.rs lines 225-415).
- **OpenHands** software-agent-sdk (commit 2f2cd2b, MIT): `openhands-sdk/.../critic/base.py`, `impl/agent_finished.py`, `impl/empty_patch.py`, `impl/pass_critic.py` in full; `openhands-workspace/.../docker/workspace.py` lines 205-270. NOT READ: the API critic (`impl/api/`), the agent server, the remote workspaces.
- **Cline** SDK (commit e68a6dd, Apache-2.0): `sdk/packages/core/src/extensions/tools/command-guard.ts` lines 1-140 of 521. NOT READ: the rest of it and the tool runtime.
- **CodeQL**: engine closed; one documentation page ("Testing custom queries") read, REPORTED not VERIFIED.

## What the references showed (VERIFIED in the code above unless marked)
- **SWE-bench's grading rules are a list of past spoofing and false-pass bugs.** An empty set of parsed results is not a pass: the log must show evidence the suite ran, with non-zero counts (karma printing "Executed 0 of 0" had scored unrun suites as resolved). The log is cross-checked with the test command's own exit status, recorded outside the parsed region, because a patch can print its own "PASSED" lines. A skipped test that should have failed is not a fix. Environment faults (browser failed to launch, out of memory, no network, module not found, no tests collected, timeout) are classified separately and never scored as a wrong patch. The tests are the project's own and a patch cannot change them: test files are reset to the trusted version before the run. Failing tests that must now pass, and passing tests that must still pass, are both required.
- **Its weakness:** about fifty per-project log parsers keyed by test name (a name repeated in two files overwrites; the Rust parser ignores `ignored`; truncated parameter ids need a prefix hack). Parsing arbitrary runner output does not scale. Our reproducers print markers we control, which avoids that, but the project test oracle does parse runner output and must be written with these lessons.
- **Agentless** decides a fix "resolved" when the whole log merely *contains* "Issue resolved" and none of the other phrases, from a single run: no negative control, no repeated run, no exit-code check, and the log also contains repository text from `git show` and `git diff`, so matching text can spoof it. Its three-state prompt is the same idea as our REPRODUCED / CONTROL_OK.
- **Semgrep** tests every rule against positive and negative examples and requires the reported set to equal the expected set exactly, so extra hits fail too. Ours is the executable form: the normal-behaviour check plus the emptied-file control. Its autofix test needs a stored expected file, which model-written fixes do not have.
- **OpenHands'** critic is a heuristic (finished with a non-empty patch) or a trained model that predicts success; it never runs the code. **Cline's** command guard calls itself a blacklist that "will not catch every possible mutation". Neither is evidence in our sense.
- **CodeQL** (REPORTED): queries are validated against exact stored expected results per language.

## Ours compared
- Stricter than Agentless on every point listed. At least equal to SWE-bench on: exit code must be 0 (with `CONTROL_OK` required, so "never ran" is not a pass); markers are whole lines, not substrings; two runs must agree; negative control with the cited file emptied; timeouts and limits are a failure, never a pass.
- Gaps the reading found in ours, now fixed (tests c23, c24, f08, f09, all mutation-checked):
  1. A fix could edit any imported file, including test files. A fix that weakens the tests it must pass proves nothing, so a fix touching a test file is now refused (`is_test_path`: test file names, `conftest.py`, anything under `tests`, `test`, `__tests__`).
  2. We compared only pass versus fail. The count now matters: a fix after which fewer tests run, none run, or the run fails is `BreaksTests`; more tests is fine (`compare_test_runs`).
  3. A missing third-party package read as a failed reproducer. SWE-bench separates "the environment broke" from "the code is wrong". It is now `Unavailable`, naming the package, unless the missing name is part of the project itself (then the reproducer is wrong, `Failed`). Only a plain package name from the interpreter's own error is ever shown.
- **Known limit, same as every reference (NOT fixed):** code under test runs in the same process as the reproducer and can print the marker words itself. The negative control catches code that prints them *only because it is there* only when the reproducer fails without it; a cited file that prints both markers at import can still produce a false "reproduced". The owner always sees the reproducer and its output. A per-run secret word was considered and excluded for now: code in the same process can read it from the environment, and it would make small local models far less reliable at writing reproducers.
- **Same limit for the project test oracle:** a changed module can print "Ran N tests / OK" and exit 0. The exit status check (SWE-bench's guard) is there, but a forged zero exit is possible; the fix diff is shown to the owner.

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
