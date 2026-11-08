# Reproducers beyond Python

## Status (honest)
Reading is done (see "References read"). Built and tested: a `language` field on a finding; the JavaScript closure and runner; and, from the reading, three corrections to fix checking that apply to both languages (a fix may not change a test file, the project's tests must still all run, and a missing package is "cannot check here", not a failed reproducer). Also built: the JavaScript project-test oracle (`node --test`, read the way SWE-bench's rules require) and a rule that a fix is accepted only if the script reaches its explicit `NOT REPRODUCED` verdict. Also built: the JavaScript release package, the prompt and the worker's `language` field, the language shown in the web and terminal screens, and an optional readiness entry for Node.js. Measured on the real local model: it writes no reproducer at all in either language (see "Measured on the real local model"), so the quality of model-written JavaScript reproducers is BLOCKED on a stronger model (NEAR AI Cloud credits); everything around it is built and proven with a scripted model through the real worker and sandbox. No claim of "best" or "comparable to the strongest tools" is made; the comparison below says where ours is stricter and where it is not.

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
- **A fourth gap, found by thinking through SWE-bench's "never ran" rule and proven by a failing test before the fix (f10):** a fix that simply ended the program early (`sys.exit(0)`, `process.exit(0)` inside the code under test) was accepted as `FixesTheReproducer`, because only the *absence* of `REPRODUCED` was checked and `CONTROL_OK` had been printed before the exit. The model is already told to print `NOT REPRODUCED` otherwise; a fix is now accepted only if the script prints that verdict line after the change. A script that omits it makes its fixes `NotChecked`, which fails closed; how often real model-written scripts omit it is to be measured (step 5).
- **JavaScript test oracle (n01, n02; formats captured from Node 22.22.1, not assumed):** `node --test --test-reporter=tap --test-concurrency=1` in the same sandbox. The runner's own summary must be present, complete and add up; failures, cancellations and the exit status must agree with it; skipped and todo tests are not passes; a run where nothing passed (nothing ran, or everything skipped) is `NoTests`, not a pass; the pass *count* is compared before and after (a module that ends the program while the test file loads makes the file count as one passing test, so fewer tests ran: caught). A test file that prints plain `tests 5 / pass 5 / ...` produces column-0 lines that look like a summary (Node only prefixes them with `# `), so the summary is accepted only as the last `# tests N` directly after the runner's plan line `1..N`; a printed plan line reads `# 1..5`. A missing package is named in the failure but never turns a failure into "unavailable" (a fix could print that text). Same limit as Python's: the oracle trusts the runner process's own report.
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
6. **Release (built, as changed during the build)**: one `run-check.py` for the whole release, chosen per finding by a `language` entry in `index.json` (absent for Python, so Python releases are byte-for-byte what they were). A JavaScript finding is released as `repro-N.mjs`; its `finding-N.txt` says the check needs Node.js 22 or newer. The recipient's runner finds `node` on the path; without it that finding prints `MISSING ... needs Node.js`, the other findings are still checked, and the exit code is 2 (1 means a check failed, 0 all passed). It clears `NODE_OPTIONS` and `NODE_PATH`, refuses a sample path that leaves its folder, and reads and writes UTF-8 explicitly. The capsule's claims are now built from the release's own `index.json` (`claims_for`) instead of assuming `repro-N.py` for the Nth confirmed finding, which would have named wrong files as soon as a finding was left out or was JavaScript.
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
- The recipient's runner reproduces the verdict on a machine with Node and says clearly when Node is missing (rj01, rj02; e01 runs the real sandbox result through the recipient's own `python3` and `node` in the VM).
- A mixed project (Python and JavaScript findings) keeps each finding's language; Python behaviour is byte-for-byte
  unchanged (all existing `check`, `job`, `release` tests pass untouched).
- The model prompt change is checked against the real local model on a fixture: whether it picks the right language and
  writes a reproducer that passes `CONTROL_OK` is measured and the rate recorded, not assumed.

## Release rule found while building it
A reproducer must find the project through the current folder, never through the sandbox's own paths (`/sample`, `/repro`): nobody else has those paths, so the recipient could not re-run it. `names_sandbox_path` makes such a script `Unavailable` with the reason, and `assemble` leaves it out of the release and lists it as not confirmed (c25, rj03). The old JavaScript test fixtures used `/sample/` and were rewritten to `process.cwd()`. Limit: a string such as `process.cwd() + '/sample'` cannot be told apart from the sandbox path without parsing the language, so it is also refused (fails closed, a rare false "cannot check").

## Measured on the real local model (VERIFIED by running; `j04`, `j05`, `KK_MODEL`, `KK_RUNS`)
- Setup: Ollama `qwen2.5:3b`, the full review pipeline (real worker binary, real sandbox in the Linux VM), 3 runs per project, one Python project and one JavaScript project with the same kind of defect (`fixtures/synthetic-repo`, `fixtures/synthetic-js-repo`).
- Python project: 3 findings in 3 runs, **0 with a reproducer**, so 0 confirmed. JavaScript project: 30 findings in 3 runs (about ten per run, against one per run on the Python project), **0 with a reproducer**, 0 with a language named, so 0 confirmed. Every finding was reported "unavailable: the finding has no reproducer", which is the safe outcome: nothing unproven is called confirmed.
- What this does and does not show: the small local model does not follow the reproducer instructions in either language, so the limit is the model, not the JavaScript path. It also reports far more findings on the JavaScript project (not measured further: whether those are real defects or false positives). The question this document asked ("does a small local model write good JavaScript reproducers at all") has the answer **no, it writes none**. Whether a stronger model writes passing ones, and how often it prints the `NOT REPRODUCED` verdict line, is **NOT MEASURED**: BLOCKED on a stronger model (NEAR AI Cloud credits); no larger local model fits on this disk. Re-run `KK_MODEL=<model> KK_RUNS=5 cargo test -p kk-review --test job j04 -- --ignored --nocapture` (and `j05`) when one is available; the output line starts with `MEASURE`.
- Proven instead, with a scripted model through the real worker binary and the real sandbox (`j09`, `j10`): a JavaScript finding goes from the model's tool call to a sandbox-confirmed result, a fix checked against the project's own Node tests ("3 test(s) passed before and after"), and a release that carries `repro-1.mjs`; an unknown language is bounced back to the model and records nothing; a script whose language does not match its labelled language is never confirmed.

## Screens and readiness
The language is stored with each finding (reviews stored before the field read as Python), returned by the findings API, shown in the web ("Reproducer the kernel ran (JavaScript)") and the terminal ("Checked with JavaScript"); an unknown value is never echoed. Readiness has an optional entry for Node.js 22 or newer: a failing optional entry is labelled Optional with its fix and does not make the machine "not ready" (only a JSON `false` in `required` makes an entry optional). Hindi strings for the new labels are unreviewed by a native reader (already an owner item).

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

## TypeScript (ledger E-C7): built with Node's own type stripping; the packaged Node.js cannot do it
- Source read: Node.js's TypeScript page, https://nodejs.org/api/typescript.md, in full (VERIFIED by reading). Type stripping is on by default from 22.18 and 23.6, stable from 24.12 and 25.2; it removes annotations and does no type checking; it ignores `tsconfig.json`; it refuses what needs code generation (`enum`, `namespace` with code, parameter properties, decorators) with `ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX`; it requires the file extension in every import (`./x.ts`, not `./x`); `.mts` is always an ES module; TypeScript inside `node_modules` is refused; it needs WebAssembly.
- **Found by running it, not in the docs (VERIFIED):** the sandbox virtual machine's Node.js, the Ubuntu package at `/usr/bin/node`, reports version 22.22.1 and `process.features.typescript === false`, and cannot run a `.ts` or `.mts` file at all. The official build of the same 22.22.1 from nodejs.org reports `strip` and runs them. So a version-number test would have said "TypeScript works" on a machine where it cannot. Capability is therefore asked of Node itself: `node -p process.features.typescript` must print `strip` or `transform`.
- The error a Node without the parser gives depends on the script (an `.mts` with an `import` fails with `ERR_UNKNOWN_FILE_EXTENSION`; one with only top-level `await` fails with a plain `SyntaxError` from the CommonJS loader), so the check does not read it from a failed run. `run_reproducer` runs the capability probe once in the sandbox before the three rounds and reports `Unavailable`, naming Node.js 22.18 and the official build, when the answer is anything else.
- Built:
  1. `Language::TypeScript`, named `typescript`, accepted by the finding parser, the worker's tool schema (and refused for `ts`, `TypeScript`, `tsx`), the stored finding, the terminal ("Checked with TypeScript") and the web ("Reproducer the kernel ran (TypeScript)").
  2. `ts_closure`: relative imports followed to the `.ts .mts .cts .js .mjs .cjs` family, `./x.js` standing for `./x.ts` (and `.mjs` for `.mts`, `.cjs` for `.cts`) as the build tool's convention has it, `index` files, type-only imports, dynamic imports and re-exports. Packages are not followed. A withheld (Restricted) file in the closure refuses the check, exactly as for Python and JavaScript.
  3. The reproducer is `repro.mts` (an ES module run with Node.js itself, no compile step) and loads project files with `await import(process.cwd() + '/src/auth/session.ts')`. The same three runs, the same emptied-file control and the same marker rules as every other language.
  4. Three fixed sentences chosen from Node's own error codes, never text taken from the error: an enum, namespace or parameter property "needs a build step"; an import without its extension "needs a build step"; a Node without TypeScript "is too old". All three make the check `Unavailable`, never `Failed` and never `Reproduced`. A project that is not erasable TypeScript is said to be unreachable, not wrong.
  5. The project's own tests: `node --test` over `.test.ts` files, run before and after a fix with the same strict reading of the runner's summary (`n03`: 3 pass, a fix that deletes only one token passes, a fix that clears all tokens is `BreaksTests`).
  6. Release: `repro-N.mts`, `"language":"typescript"` in the index, and the recipient's `run-check.py` first asks the recipient's Node.js the same capability question and prints `MISSING ... use the official Node.js 22.18 or newer` (exit 2) when the answer is not `strip` or `transform`, instead of a false failure.
  7. Readiness: the "JavaScript and TypeScript checks" entry says whether this Node.js has TypeScript. `KAKU_NODE` (a plain absolute path as seen inside the sandbox, default `/usr/bin/node`) selects the Node.js for both languages. `scripts/provision-lima-node` installs the official build into the test VM at `/usr/local/bin/node` after checking its SHA-256 against the `SHASUMS256.txt` published beside it (the file comes from the same server over HTTPS; it is not a signature check); `scripts/test-lima` runs it first.
- Evidence: kk-review ts01-ts04 (parsing, closure spellings, error classification with texts captured from both builds, the capability words), c30 (real VM sandbox: a real defect Reproduced with exactly the three files it reaches and not the `.env`; decoy NotReproduced; no-control, independent and emptied-file cases Failed; network, file read, file write, throw, nondeterministic, an `enum` in the script, an enum in the project, an extensionless import, and the packaged Node.js all not Reproduced and the last three Unavailable with their sentence), n03 (project tests and fix verification), rj05 (release and recipient, including a stand-in `node` that answers `false`), j15 (a scripted model through the real worker to a checked fix and a release carrying `repro-1.mts`), j16 (a TypeScript script labelled javascript or python, and a Python script labelled typescript, never confirmed), w01/w02 (worker schema), nc02-nc04 (readiness and the path rule), tu (terminal label) and a browser check (442: TypeScript and JavaScript labels, an unknown name never repeated). 20 mutants all caught (15 without the VM, 5 only the VM test can catch).
- Fixture: `fixtures/synthetic-ts-repo`, a TypeScript twin of the JavaScript one (explicit `.ts` imports, annotations only); its three tests pass under `node --test`.
- Limits (NOT covered): code that needs a build step (enums, decorators, path aliases from `tsconfig.json`, extensionless imports, JSX) is `Unavailable` rather than checked; no type checking is done, so a type error is not a finding; a stronger model's ability to write TypeScript reproducers is NOT MEASURED (the small local model wrote none for Python or JavaScript either); the capability probe asks the same Node.js that then runs the script.
- Rust is still owed (a different toolchain: `rustc` or `cargo` offline in the sandbox and a module-tree closure); E-C7 stays PARTIAL for that reason only.

