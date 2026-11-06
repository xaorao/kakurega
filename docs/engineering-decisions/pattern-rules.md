# Pattern rules: a second opinion on findings (ledger F-I7)

## Target experience
- A reviewer who wants more than the model's word and the reproducer can switch on a scanner that looks for code of known shapes, and sees its results next to each finding, labelled for what they are.

## References and what was read
- Semgrep 1.136.0, installed and run by me in a throwaway virtual environment (VERIFIED). Licence: the engine is LGPL-2.1 and is only executed as a separate program (never linked or bundled); its registry rules have their own licence and are not used. Documentation read: its CLI behaviour was learned by running it (flags `--metrics=off`, `--disable-version-check`, `--disable-nosem`, `--no-git-ignore`, `--json`, `--timeout`, `--max-target-bytes`, `--config`). NOT READ: its source and its full documentation.
- Observed by running it: inside a git repository it scans only tracked files and skips `tests/` by default (so a copy in a fresh, non-git folder with an empty `.semgrepignore` is needed); with a local rules file and `--metrics=off` it needs no network; a rule id comes back prefixed with the folder the rules were read from.

## Decision
- BUILD, optional, off unless the owner sets `KAKU_SEMGREP` to the full path of a semgrep (no `PATH` search, so running a scanner is a deliberate act). Rules are ours: 13 rules in `crates/kk-review/rules/patterns.yml` (Python: eval/exec of a non-literal, shell=True, os.system/popen, pickle, yaml.load without a safe loader, md5/sha1, TLS verification off, SQL built from text, hard-coded credential names; JavaScript/TypeScript: eval/new Function, shell exec, innerHTML/document.write, md5/sha1). The sentence the owner sees for each rule is read from our file; text returned by the scanner is never shown.
- Handling of an untrusted project: only files the model may also see (nothing labelled restricted) are copied, with safe relative paths only and at most 1 MB each and 5000 files, into a fresh private folder; an empty `.semgrepignore` replaces any the project carried; `--disable-nosem` so a comment in the project cannot hide a line; empty environment except a throwaway `HOME` and the three switches that turn off metrics and version checks; own process group, killed with its helpers after 120 s; output capped at 8 MB; the answer is believed only for our rule ids and the files we gave it, at most 500 matches, and a non-zero exit, unreadable answer or flood is a stated failure, never a result.
- What the owner sees: `/findings` (and `GET /api/jobs/{id}/findings`) show, per finding, the pattern matches in the same file ("Pattern rules also matched in this file (code of that shape; not a reproduction)") and one line about the whole run, or the reason it could not run. A pattern match never changes `status` (reproduced / not reproduced), the release, or the approval.

## What running it showed (VERIFIED, real semgrep 1.136.0)
- The sample files: 10 expected matches in the bad Python file and 4 in the bad JavaScript file; none in the good counterparts (parameterised queries, safe loader, sha256, literals, empty and counting names).
- The synthetic fixture project (the real logout-does-not-remove-the-token defect and its decoys): zero matches. That confirms the matrix's inference: pattern rules do not find logic bugs and do not "confirm" decoys either. They are a second opinion about shapes, not a check of the finding.
- `# nosem` and a project `.semgrepignore` of `*` do not hide anything; a file under `tests/` is scanned.

## Evidence
- kk-review pt01–pt05 (rules file, setting validation, believing only checked answers, flood cut-off, safe paths), pp01–pp04 against a stand-in scanner (arguments, emptied environment, files given, empty ignore file, hang with helper killed, output flood, bad exit, missing program, big files, working folder removed) and pp05 against the real one, j12 through a whole review in the VM (restricted files are not handed over; the finding stays as the reproducer decided), kk-service sv111, kk-tui tu63. 16 mutants caught.
- pp05 runs only when `KAKU_TEST_SEMGREP` names a semgrep; otherwise it prints NOT RUN. The gate does not set it, so the gate does not exercise the real program; I ran it by hand with the result above.

## Not covered / owed
- Semgrep on a Linux host or inside the sandbox VM (it runs on the owner's computer, like the other readers; running it in the sandbox would need it installed there). Windows. The web review screen does not show the matches yet. The rules are a starting set, not tuned on real projects, and there is no way yet for the owner to add their own rules. Whether the second opinion changes any owner decision is NOT VERIFIED.
