# Durable small files (ledger G-GH-88)

## Target experience
Pulling the plug, a crash or a full disk during a save never leaves a settings or key file half-written. After it, the product starts from the old file or the new file, and the one file that unlocks every encrypted store can always be recovered from a copy.

## Reference and reading depth
Ghast companion_service (MIT, commit 6098aec): FULL read of `companion-runtime/src/persist.rs` (primary and `.bak` files, `NamedTempFile` write and persist, 0600 files and 0700 directory, load falls back to the backup when the main file fails to parse) and `companion-config/src/lib.rs` lines 505-545 and 980-1016 (`save_config` through a temporary file, permissions set before and after the rename). Not read: `companion-checkpoint`, `companion-control`, `companion-memory`, `companion-cron`, `companion-browser` (they were listed as users of the same pattern).

## Weaknesses of the reference, fixed in ours
- No `fsync` of the data or of the directory before and after the rename, so a power cut can leave a renamed but empty file. Ours forces the file to disk, then the directory entry.
- The backup is a plain `fs::copy` of the main file, so a crash during the copy leaves a cut-short backup. Ours writes the backup the same atomic way.
- Permissions are set after the data is written and again after the rename. Ours creates the file with the mode and sets it explicitly (the umask cannot narrow it), so the content is never readable by others at any moment.
- Reading falls back to the backup only when the main file fails to parse, and nothing says the main file was put back. Ours takes a validity check from the caller, restores the main file from the backup, and reports damage with no usable copy as an error instead of returning empty state.
- A leftover temporary file from a crash is never cleaned. Ours removes this file's leftovers once they are an hour old and never touches another file's or a recent one.

## What was built
- `kk_vault::durable`: `write_atomic`, `write_atomic_with_backup`, `read_checked`, `create_atomic` (create only if absent; of two racing callers one wins, by hard link), `backup_path`. A symbolic link at the target is replaced, never written through.
- `vault.key` (the key to every encrypted store): created atomically with `create_atomic` and a copy `vault.key.bak` (same mode 0600, the same secret: guard it like the key); read with a check that it is 64 hex characters; a key file that is empty, cut short or gone is put back from the copy. A key that is damaged with no usable copy is replaced only when no database exists beside it (a first start that crashed is no longer a dead end); with a database beside it the error is reported and nothing is overwritten, because a new key would lose the data for good.
- A restore writes the recovered key atomically, last.
- The Telegram bound chat and the update "seen" marker are written atomically with mode 0600 (the bound chat file was written with the default permissions before).
- Already atomic and left as they were: the delivered release (`deliver.rs`), fix files (`fixfiles.rs`), the Telegram status file and the update root (write to a temporary file, then rename; no fsync).

## Evidence
kk-vault du01–du07 (exact bytes and mode, umask, failure leaves the old file and no temporary file, links, backups, restore from the backup, leftovers, eight writers and a reader never see a partial file); kk-service ky01–ky05 (one key and a copy, twelve programs starting together get one key in 20 rounds, a damaged key is restored from its copy, a first-start crash is not a dead end, a damaged key beside a database is reported and untouched); kk-cli tg60. 20 mutants, all caught except the fsync call, which no test can observe without cutting power.

## Not covered
- A real power cut: durability across it rests on `fsync` and is not demonstrated here (NOT VERIFIED).
- Windows (the module uses Unix modes and hard links).
- Files written by other code paths that were not listed above (the audit covered `fs::write` and `OpenOptions` in `crates/*/src`; scripts and the desktop shell were not audited).
