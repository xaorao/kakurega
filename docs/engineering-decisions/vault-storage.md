# Encrypted storage (experiment X5)

## Target experience
Private data on disk is unreadable without the key, including indexes, the write-ahead log, backups and anything left behind by a crash. A wrong key, a foreign file or a damaged page is reported, never silently read.

## References
- SQLCipher (commit 355d4db, BSD-style licence), README and `src/sqlcipher.c` read for build requirements (HAS_CODEC, `SQLITE_TEMP_STORE=2` or `3`) and migration/journal handling. Quest and Ghast were observed storing plain SQLite (Ghast: plaintext DBs on disk).
- rusqlite 0.41.1 with `bundled-sqlcipher-vendored-openssl` (SQLCipher 4.19.0 community, OpenSSL built from source, no system library dependency).

## Approach
`kk-vault` opens SQLCipher databases only with a raw 256-bit key (hex key form, no passphrase KDF cost; key derivation from a passphrase or the OS keychain is a separate layer above). Every connection sets `cipher_memory_security`, `temp_store=MEMORY`, `secure_delete`, WAL with `synchronous=FULL`, and then installs an SQLite authorizer that refuses application code from changing the temp-store mode or directory, the key, the journal mode, or from attaching other databases. Backup (`VACUUM INTO`) and re-key lift the guard briefly inside the vault and restore it.

## Results (all VERIFIED_BY_RUN, `cargo test -p kk-vault`)
- A vault file has no plaintext SQLite header; a plain SQLite open cannot read it; a wrong key is refused.
- Canary scan (three encodings: UTF-8, UTF-16LE, hex) of the database, WAL, shm and the backup after 400 rows with an FTS5 index, search, sort and checkpoint: not found. Scanner control: the same scan does find the canary in a plain database.
- 30,000-row sort and temp table workload with a small cache and a clean `TMPDIR`: no plaintext in the data directory or the temp directory.
- Crash (process abort inside an open transaction): no plaintext afterwards, reopen succeeds, only committed rows remain, `integrity_check` is ok.
- Re-key: old key refused, new key works, no plaintext.
- A 64-byte corruption in the middle of the file is detected on read (SQLCipher per-page HMAC).
- The authorizer refuses `PRAGMA temp_store = FILE`, `temp_store_directory`, `rekey`, `cipher_memory_security = OFF` and `ATTACH`.

## Limits and open items
- SQLite unlinks its temp files as soon as it opens them, so file listing cannot show them. The defence is the compile-time `TEMP_STORE=2` (verified via `PRAGMA compile_options`) plus the guard; spill behaviour with `temp_store=FILE` is deliberately not exercised.
- Swapped backup with the same key (VT03) and second-device recovery (VT04) need a vault identity and a monotonic generation record; not built yet.
- Plaintext in RAM and swap, and OS backups of an unlocked machine, are outside this layer (stated in the threat model).
- Key derivation and OS keychain storage are not part of `kk-vault` yet.
