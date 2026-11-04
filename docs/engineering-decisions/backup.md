# Encrypted backup and restore

## Target experience
- One command makes a single file holding everything the product knows (memories, review history, the signed event log, approvals, the signing keys). The owner is shown a recovery key once.
- With the file and the key, a new computer gets the same state. With the file alone, nobody can read anything.

## References
- Ghast `companion_service/crates/companion-checkpoint/src/sync.rs` (MIT). READ: lines 1-215 (constants, versioned envelope, key derivation, AES-GCM with associated data, encrypt and decrypt). NOT READ: lines 215-603 (stream encoding, write order, remote storage), which concern uploading to a remote store and do not apply to a local file.
- What was taken as behaviour: a versioned envelope that names its algorithm, a random nonce, and context bound as associated data so a payload cannot be moved to another context.
- Weaknesses of the source addressed: its key is derived from a wallet private key, so a backup key and a spending key are the same secret; its salt and info strings are constants, so one key encrypts every payload; a missing KDF field makes it try two derivations in turn (a downgrade path).

## Decision
- REBUILD (no code copied): `crates/kk-backup` — a random 256-bit recovery key, written as `KKRK-` plus base32 groups with a check so a typo is caught; a fresh random salt per backup expanded by HKDF-SHA256 into a one-use key; ChaCha20-Poly1305; the whole header (magic, version, salt, nonce, creation time) is associated data; exactly one format and no fallback; a strict archive (names lowercase `[a-z0-9._-]`, no duplicates, per-entry SHA-256, no trailing bytes, size caps).
- `crates/kk-service/src/backup.rs`: databases are copied with SQLite's own copy while the service runs; the vault key travels inside the sealed file. Restore only writes into a folder holding no Kakurega data, stages first, opens every database with the backup's own key before anything is moved, never overwrites, and refuses unknown file names.
- CLI: `kaku backup [--out FILE]`, `kaku restore FILE --data-dir DIR` (key from `KAKU_RECOVERY_KEY`, `--key-file`, or typed). The output file is mode 0600 and an existing file is never replaced.

## Limits (stated)
- NOT BUILT: the web, desktop and terminal-workspace surfaces for backup (CLI only); scheduled backups; incremental backups; a passphrase option (the recovery key is the only secret, and losing it loses the backup); restoring into a folder that already has data (deliberately refused); backing up `admin.json` and the scratch folder (re-created on start).
- The recovery key is printed to the terminal once. Terminal scrollback or shell logging may keep it; this is stated to the owner.
- Not tested: backups above a few hundred megabytes, concurrent heavy writes during the copy, restore on another operating system.

## Acceptance
- `cargo test -p kk-backup` (bk01-bk07), `cargo test -p kk-service --test flow sv27`, and a manual run of the real commands. Mutations failed tests: header not authenticated on both sides, key check removed, entry checksum removed, trailing bytes accepted, duplicate names accepted, restore into a non-empty folder, database verification replaced, unknown file names accepted.
