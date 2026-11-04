# Memory (registry rows G51-G54, G57, N14; G58 later)

## Target experience
Kakurega remembers what the user wants remembered, can say why it remembers it and since when it is true, never hands a memory to a route that is not allowed to see it, lets the user correct it without losing the history, and when the user says forget, the text is gone from disk and cannot come back by itself.

## Strongest references and what was read
- Anamnesis (Apache-2.0, commit 9d89436, `private-ai/ghast-ai/Anamnesis`): read `GOAL.md`, README, the store migrations 0001 (records, FTS5 with triggers), 0007 (tombstones keyed by the source's natural key so a re-import cannot resurrect a forgotten record) and 0009 (user overlays kept apart from derived data). NOT read: the 14 adapters, the hybrid search and packer (about 2000 lines), the MCP server. Its stance is an interoperability bridge that never edits sources, which is the opposite of ours, so no code is adopted; ideas adopted: tombstone that blocks resurrection, provenance on every record, overlays separate from derived data.
- ghast-memory-os (Apache-2.0, `src/kernel/safety.ts`, `temporal-validity.ts`, `retention.ts` skimmed): ideas adopted: secret-like content never becomes long-term memory; sensitive memory hidden from ordinary context; supersession (`supersededBy`); validity windows. Its date extraction engine (multi-language regex) is not adopted: validity is set explicitly by the user or the kernel, not guessed from prose. NOT read in full: reconstruction, consolidation, gym, retrieval packing.
- Weaknesses to beat: both keep forgotten text recoverable from storage pages or FTS shadow data unless carefully cleaned; neither labels memories for a privacy kernel. Ours: SQLCipher vault with secure_delete and WAL truncation, FTS delete, tombstone as a keyed hash (no plaintext), and a label on every memory enforced at recall.

## Chosen approach
New crate `kk-memory` on the encrypted vault: `memories` table with label (level, compartments, integrity, unknown-sensitivity), validity window, subject, reason and source, status (active, superseded, forgotten), FTS5 over the text; `memory_events` audit without plaintext for forgets; `tombstones` keyed by HMAC of the normalised text. Recall takes the caller's clearance label and an as-of time; memories above the clearance are not returned and not counted.

## Quality criteria (acceptance tests me01-me12 in `crates/kk-memory/tests`)
Label gate (including unknown sensitivity as Restricted); time validity both directions; correction keeps history; forget leaves no plaintext in the database file or WAL; tombstone blocks the same text in any casing or spacing and stores no plaintext; secret-like text refused; duplicates not added; FTS syntax in a query cannot error or widen results; limits and control characters refused; reopen with the key works and a wrong key fails.

## Not covered by this component yet
Semantic (vector) recall, memory import (G55), conflict alerts (G56), encrypted backup and recovery flow (G58, the vault has `backup_to`), automatic capture from reviews, UI.
