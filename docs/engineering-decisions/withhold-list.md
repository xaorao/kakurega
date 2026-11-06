# The owner's withhold list: `.kakuignore` (ledger row F-S8)

## Target experience
- The owner lists, once, files and folders that must never be part of any review. They are withheld from the model and from the release exactly like secrets, without the owner having to move or rename anything.
- The list is easy to write (it reads like a `.gitignore`), cannot be silently undone by a later line, and fails closed.

## Sources
- gitleaks and detect-secrets allow-list files and `.gitignore` semantics (public behaviour; no code used). Reference reading for the secret detector is recorded in `secret-detection.md` and the trace (row 29); the matrix row F-S8 asked for an owner-declared list honoured by the importer.
- Decision: write a small matcher of our own instead of adding the `ignore` crate, because the list must be fail-closed (no negation, no escapes that change meaning, hard caps) and every extra dependency widens the trusted core.

## Behaviour
- File: `.kakuignore` at the top of the project. One pattern per line; blank lines and `#` comments are nothing. `name` matches a file or folder of that name at any depth and everything inside a matching folder; a `/` at the start or in the middle ties it to the top folder; a trailing `/` means a folder; `*` and `?` stay inside one name; `**` as a whole part is any number of folders. Case is ignored, so a list written on a case-insensitive disk still holds.
- Never supported on purpose: a line starting with `!` is ignored and counted (nothing can be un-withheld). Escapes and `[a-z]` classes are literal. At most 500 patterns of at most 200 characters; the rest are ignored and counted. Matching is dynamic programming over the parts of the path with a linear star matcher, so a hostile list cannot make it slow (ki03).
- Effect: a matching file gets the label Restricted with the compartment `owner-withheld` and the note "withheld by your .kakuignore"; it is counted as withheld, never sent to the model, and its values are part of the release egress check like any withheld file.
- The list itself is never part of the review (skipped with the reason "your withhold list (never sent)").
- A list that cannot be trusted to say what is private (a symbolic link, a folder, larger than 64 KiB, not text) withholds every file, with the note "your withhold list could not be read, so everything is withheld". A symbolic link is never followed.

## Evidence
- kk-review `ki01`-`ki07` (28 pattern cases, comments and negation, hostile lists and timing, the importer with a real tree, unreadable lists, a symbolic link, no list). Live: `tests/e2e/mcp_live.py` runs a real sandboxed review of a copy with a `.kakuignore` and the status reports 3 withheld files (the secret and the two listed). 14 mutants of the matcher and importer: 13 caught; one is equivalent (`(0..=` versus `(1..=` over the prefix lengths cannot matter because a pattern always has a part).
- Not covered: very large projects with a list of 500 patterns (the cost is patterns times files times path depth, not measured at scale); a list inside a sub-folder (only the top-level list counts).

## Owed
- Show the list's effect in the web protection tab beyond the per-file note; a list in sub-folders; the same list honoured by the browser extension.
