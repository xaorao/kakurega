# Signed updates

## Target experience
- The owner is told when a new version exists, sees what it changes, and installs it only by their own choice.
- A forged, replayed, stale or swapped update can never be accepted, even if the download site or the network is hostile.
- If a publisher key is stolen, the owner's installations recover without a reinstall.

## References read (by me)
- **Tauri updater** (`tauri-apps/plugins-workspace` commit 685d342, MIT or Apache-2.0): `plugins/updater/src/updater.rs` lines 494-1010 and 1885-1975 (the check, the download, the signature and signed-version checks) and `config.rs` lines 95-150 and 198-221, of 2,879 and 221 lines. NOT READ: the install paths per platform, `lib.rs`, `commands.rs`.
- **python-tuf** (`theupdateframework/python-tuf` commit 31bba85, MIT or Apache-2.0): `tuf/ngclient/_internal/trusted_metadata_set.py` in full, `ngclient/config.py` in full, `ngclient/fetcher.py` lines 60-140, `tuf/api/_payload.py` lines 262-330 and 418-497 (role keys, threshold counting). NOT READ: `ngclient/updater.py` beyond a search for its limits, the TUF specification text, delegations.
- **Sparkle** (`sparkle-project/Sparkle` commit 14fdbf2, MIT): `Sparkle/SUUpdateValidator.m` lines 225-419 of 419 (key rotation and the policy that keys may be rotated but never removed). NOT READ: appcast parsing, version comparison, phased rollout, the installer.
- Our own implementation, `crates/kk-update` and `crates/kk-fetch`, read in full before changing it.

## What the references showed (VERIFIED in the code above)
- Tauri signs the PACKAGE, not the manifest. By default a hostile endpoint can pair a newer version number with an older, validly signed package; their opt-in `require_signed_version` binds the version into the signature, and their own comment says leaving it off lets an attacker bypass the check by serving a signature that carries no version. It also has no record of the highest version seen, no expiry, buffers the whole download in memory before checking the signature (no size limit), and only warns for a non-https download address.
- TUF's rules: strictly increasing versions per role, expiry on the final metadata, a root replacement must be version+1 and signed by the old threshold AND its own threshold, a cap on rotations followed, a hard upper bound on every download enforced while streaming, thresholds count distinct keys with duplicate key ids refused and a threshold below one refused.
- Sparkle: an update is accepted if the OLD EdDSA key or the OLD code-signing identity validates it (so one can rotate while the other stays), the check is always against the old key, and keys may be rotated but never removed.

## Ours compared (before this work)
- Stronger than Tauri by design: the whole announcement is signed (version, expiry, channel, checksum, size), there is an expiry, and a highest-seen record defeats replays.
- Weaker than TUF: one key, no rotation, no threshold; a forged far-future version signed with a stolen key would be remembered for ever and lock out every real release; no download code with a hard cap (the check took a file already in memory).

## Decisions
| Capability | Decision | Where |
|---|---|---|
| Whole signed announcement (version bound) | KEEP ours over Tauri's unsigned manifest | `kk-update::check` |
| Threshold of distinct keys; duplicate id, zero threshold and a bad signature from a trusted key refused | ADAPT from TUF, own code | `kk-evidence::dsse::verify_threshold` |
| Signed root with rotation: version+1, old threshold AND new threshold, at most 32 rotations followed, final root unexpired, an expired middle root allowed | ADAPT from TUF, own code | `kk-update::root` |
| Recovery from a stolen key: the highest-seen record is kept with the root version it was made under and forgotten when the root changes (TUF's fast-forward recovery) | ADAPT | `root::Seen`, `kaku update` |
| Streamed download capped at the signed size, hashed as it goes, private temp file renamed only when size and checksum match, a failure never replaces an earlier good file, never opened or run | BUILD; beats Tauri (no cap, buffered in memory) | `kk-update::download`, `kk-fetch::Fetcher::fetch_to` |
| One guarded network door for the artifact too (https only, public addresses, pinned, redirects re-validated, cap enforced by the fetcher as well as the transport) | BUILD on the existing door, not a second network path | `kk-fetch` |
| Platform name limited to `[a-z0-9_-]` up to 40 characters, in the manifest and when saving | BUILD (a signed manifest must not choose a path) | `check_artifact`, `download_with` |
| Key removal policy and two independent identities for rotation (Sparkle: signing key plus Apple code-signing identity) | NOT BUILT: the second identity is the owner's Apple Developer ID | blocked |
| Download from several mirrors in order (Tauri, TUF) | NOT BUILT: no update host exists yet | owed |
| Installing the replacement app, background checks, a menu item, delta updates | BLOCKED or NOT BUILT: need a signed and notarised build | blocked |

## Commands
`kaku update keygen | sign | check | download | verify-file | root-sign | root-init | root-rotate | root-show`. Installing the first root only describes it until `--accept` is given and never replaces an installed root; a replacement goes through `root-rotate`, which saves nothing unless the whole chain verifies. `sign` and `root-sign` accept several `--key` files for a threshold.

## Limits (stated)
- BLOCKED: installing a replacement app needs a signed and notarised build (the owner's Apple Developer ID). No publisher key or root exists yet, and no update host, so nothing here has been run against a real publisher.
- NOT VERIFIED over a real update host. VERIFIED over the real network only for the streaming transport itself (`fs05`, a real https page, the cap, a 404).
- `not_after` and root expiry compare with the computer's clock; a clock set far back would accept a stale announcement, and the highest-seen record limits the damage but is not a trusted time source.
- A stolen key that is never rotated can still announce anything it likes until the owner replaces the root; rotation needs the old keys' threshold, so if ALL old keys are lost no replacement is possible and a reinstall is the way out.
- The first root is trusted because the owner accepted it; its key ids are printed for checking with the publisher, but nothing here can prove they belong to the right publisher.
- Mutant notes: removing the "the source reported an error" check changes no result, because integrity is decided by size and checksum alone.

## Acceptance
`cargo test -p kk-evidence --test threshold` (th01-th06), `cargo test -p kk-update` (up01-up09, rt01-rt07, dl01-dl06), `cargo test -p kk-fetch` (fs01-fs04 and the live `fs05` with `--ignored`), `cargo test -p kk-cli --test update` (uc01-uc03 with the real binary). Mutation checks: 8 on the threshold check, 14 on the root and check rules, 6 on the CLI, 6 on the streaming door, 9 on the download plus the one equivalent mutant noted above.
