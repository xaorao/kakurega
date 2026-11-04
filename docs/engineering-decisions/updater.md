# Signed updates

## Target experience
- The owner is told when a new version exists, sees what it changes, and installs it only by their own choice.
- A forged, replayed, stale or swapped update can never be accepted, even if the download site or the network is hostile.

## References
- Behaviour known from Sparkle (EdDSA-signed appcast), Tauri's updater (minisign-signed manifest) and The Update Framework (rollback, freeze and mix-and-match attacks). NO source was read for this slice; the design is independent and stated from those published behaviours. electron-updater was not studied.

## Decision (INDEPENDENT, reusing our own DSSE envelope and key code)
- A signed announcement is a canonical-JSON DSSE envelope (`application/vnd.kakurega.update.v1+json`) with one signature from the publisher key. Fields: channel, version (major.minor.patch only), `not_after`, notes, artifacts (platform, https url, sha256, size). Unknown fields are refused.
- Rules in `kk-update::check`: signature by a trusted key only; right channel; not expired (defeats a frozen old announcement); not older than the highest version ever seen on this computer (defeats replay/rollback); strictly newer than the running version; exactly one artifact for this computer; url must pass the same address rules as web fetching; checksum 64 lowercase hex; size 1 byte to 600 MB. Notes are shown as plain text with hidden characters removed.
- `verify_download`: exact size and SHA-256 of the signed announcement.
- CLI: `kaku update keygen|sign|check|verify-file`. Keys are never overwritten; the private key file is mode 0600. `check` records the highest version seen in the data folder. Nothing is downloaded or installed by any command.
- Trust root: `update-publisher.pub` in the data folder or `--trusted`. No publisher key is embedded because none exists yet; without one, nothing is trusted.

## Limits (stated)
- NOT BUILT: downloading in the desktop shell, replacing the installed app, a menu item, background checks, delta updates, key rotation or revocation (a single key; losing it means a new install), a threshold of several publishers.
- Installing a replacement app needs a signed and notarised build (Apple Developer ID, the owner's), so the last step is BLOCKED. This slice proves the part that decides whether a file may be trusted.
- The size check is redundant with the checksum (an equivalent mutant: removing it changes no test result).
- `not_after` compares with the computer's clock; a clock set far back would accept a stale announcement. The highest-seen record limits the damage but is not a trusted time source.

## Acceptance
- `cargo test -p kk-update` (up01-up09), `cargo test -p kk-cli --test update` (uc01 with the real binary). Mutations of the signature check, expiry, rollback, channel, duplicate artifact and checksum each fail tests.
