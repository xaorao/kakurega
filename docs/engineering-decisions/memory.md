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

## Update (capability matrix E, M11): notes for a review
A review no longer gets the 50 newest notes regardless of the task. It gets up to 25 notes within a 24 KiB budget: first the notes that match words from the project folder name and the recipient (recall per word, label-filtered in the query), then the most recent to fill the rest. Each note carries `Included because:` so the owner and the agent can see why it is there; labels and the kernel's clearance rules are unchanged. Test sv25 (mutation: matching switched off fails it). Not done: vector recall, hybrid ranking, and choosing notes from the actual file list (the list is only known inside the runner).

## Update (Ghast order item 4, matrix E M7): recall by fusion
- Reference: Anamnesis `crates/search/src/hybrid.rs` (Apache-2.0), READ in full for the fusion function (`rrf::merge`), pool sizing (`candidate_pool_for_limit`), modes and explain fields (lines 1-560). NOT READ: the 600 lines of tests and the store layer, the embedding providers.
- Decision: REBUILD from the algorithm (no file copied). Recall now merges two rankings: BM25 keywords and closeness of wording (character-triple coverage, which tolerates plurals, small typos and non-English words). Same stopword rule as the keyword path. Each recalled note says which ranking matched it.
- Beats the source in: deterministic order (the source's merge sorts a hash map by score only, so equal scores can come out in a different order each run); label, date and forgotten-note filtering inside both queries.
- NOT BUILT: real embedding vectors (needs a model: Ollama or NEAR AI Cloud embeddings; vectors would carry the note's label and die with it), a recall quality benchmark against a labelled set, rerank.
- Tests: fu01-fu05, me21-me23; mutations of the label filter and of the similarity list failed tests. A mutation of the id tie-break did not, because the ordered map plus a stable sort already gives the same order; the id clause is a second guard.
