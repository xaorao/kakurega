# Proposed fixes: several changes in one file (ledger E-C1)

## Target experience
- A real fix often touches more than one place of a file. The model can say so in one fix, the owner sees one diff with only the lines that change, applies it with one confirmation and undoes it with one click.
- Nothing about "exactly the text the owner saw" gets weaker: every change is found once in the file as it is, and nothing is guessed.

## Sources read
- OpenClaw `src/agents/apply-patch-parse.ts` and `apply-patch-update.ts` (MIT, commit f38495f7d, a port of the Codex `apply_patch` grammar): READ IN FULL (218 and 325 lines). Taken: several chunks per file are located against the original, in file order, and joined into replacements applied together; CRLF and a byte order mark are kept. Not adopted: tolerant matching of whitespace, dashes and quotes (it accepts text that is not really there), the `@@` context anchors and order dependence (ours is exact and order free), Add/Delete/Move operations (a fix here changes existing text only).
- Codex `apply_patch` itself: the local clone does not contain it (checked: no `apply-patch` crate, no `*** Begin Patch` anywhere); the grammar was known from the OpenClaw port. NOT READ: Codex's own `parser.rs` and `lib.rs`, OpenCode `edit.ts` (read in an earlier row), Cline `apply-patch.ts`.
- Found in our own code while reading: `apply_fix` counted only non-overlapping matches, so `aa` in `aaa` passed as "exactly once". Fixed and tested.

## Behaviour
- A fix is at most 8 changes in at most 4 files; several may be in one file (`also` may repeat a file). Each `old_text` and `new_text` is at most 8,000 characters, as before.
- `consolidate` joins the changes of one file into one change of the span they cover, before anything is checked: each `old_text` must be in the file exactly once as it is now (overlapping matches count), no two may share a byte (touching is fine), the order written does not matter, the file must be text, and the span from the first to the last change is at most 200,000 bytes. The joined change is the text a person would get by making the changes one by one (property test, 300 random sets). A set that cannot be joined or made is not offered.
- Everything after that still handles exactly one change per file: the checksum of the reviewed file, the saved original, applying, undoing (the text the fix wrote must still be there exactly once), crash recovery. `apply_all` now refuses a fix that names a file twice, instead of letting the second change overwrite the first.
- The diff is a real line diff of before and after with three lines of context; lines that are the same on both sides are context, and changes more than six unchanged lines apart are separate hunks. A rewrite too large to compare (over 4 million line pairs) is shown as one replacement. For every random set of changes the diff, applied to the file, gives the changed file (test f23).

## Evidence
- kk-review f20–f29 (joining, refusals, one-byte overlap, order, touching, bounds, 300-round diff round trip, hunk separation distance, huge rewrite, duplicate file), job j11 in the Linux sandbox (a model proposes a two-change fix: it is joined, checked against the reproducer and the project's own tests, shown as one diff; a fix with a missing or overlapping change is not offered), browser e2e 233/233 (the UI shows both changes, applies both, undoes to the exact original bytes). 22 mutants of check.rs and job.rs: all caught after four tests were added for survivors (one equivalent mutant).
- Weakness kept on purpose: undo of a joined change needs the whole joined text to still be there, so an edit made by hand between two changed lines blocks undo of that fix (it says so and restores nothing) instead of guessing; per-change undo would keep such edits.

## Not covered
- Hunks in a file larger than the span bound, files that are not UTF-8, deleting or creating files, renaming. The model's choice of hunks is not judged, only that they apply exactly.
