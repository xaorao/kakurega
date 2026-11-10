# Independent audit of the recovery records (RCR-I, RCR-II, RCR-III)

## What was audited, by whom
- A fresh agent that had not seen my reasoning, read-only, checked about 50 claims of `rcr-ledger.md` against the code: the eleven newest test ids (run), twelve other built rows it chose, every row closed with a TRIAGE reason (29), ten excluded rows, and one run of `scripts/exfil-grid`. Its token use (about 128,000) is in `PROGRESS.md`.
- What it did NOT verify (and I did not claim): the mutation counts, the real-model scripts and the one-off measurement in B-W26.

## Findings and what was done about each
| Finding | Check by me | Action |
|---|---|---|
| Six rows (A-A26, B-W49, B-W20, E-A3, E-A8, G-GH-18) said they waited for "the worker loop" although a task loop exists; E-A8 said "not read enough to decide" | CONFIRMED: `run_task` exists; E-A8 broke the reading rule | Read OpenCode's todo tool, its prompt, the todo helper and the plan plugin in full; decided and wrote the reasons (todo and plan mode EXCLUDED with reasons; large output handling is BUILT by bounding, with its tests) |
| G-GH-22 (voice) was EXCLUDED while its decision said BUILD local | CONFIRMED: a gap filed as a decision | Moved to LATER with the real reason (a local speech model and a microphone permission do not exist yet) |
| F-I4 and F-I5 said BUILT while `secret-detection.md` said PARTIAL; the release-versus-input asymmetry was not stated | CONFIRMED | The doc now says they were closed as one grid; the asymmetry and the ungraded web-page source are written in both rows |
| B-W26 quoted a measurement with no script | CONFIRMED | Marked OBSERVED, no script kept |
| D-D21 named profiles but evidence named flags; E-M12 evidence vague; some excluded rows repeated the decision as evidence | CONFIRMED | Reworded with the real reasons and test ids |
| A-A25 (aside while an answer arrives) rests on an untested NOT VERIFIED | CONFIRMED | Left excluded with that label stated; testing whether the local server answers two requests at once is owed |
| E-A10 did not say what replaces a critic | CONFIRMED | Reason added: the reproducer re-run with the cited file emptied is the independent check |

## What the audit confirmed
The eleven newest ids hold: each test would fail if the claimed behaviour were removed (the audit looked for assertions that pass trivially and found none). It also confirmed a dozen older rows by reading their code and a test, found 22 of 29 triage reasons reasonable and the rest dependent on the NEAR route, other platforms or the signing identity (external, not design), and ran the exfiltration grid: 10 of 10 surfaces held.

## What this does not establish
- It is one audit of a sample, not of all 372 rows.
- Tests that mock the service (the terminal command tests) prove the wiring; the real-binary runs prove the rest and were listed in each row.
- 79 rows are LATER: they belong to components that do not exist yet (agents roster, plugin store, computer use, hosted mode and similar) or need credits, a second device or a signing identity.
