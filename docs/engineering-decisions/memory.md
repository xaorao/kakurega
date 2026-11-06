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

## Project-scoped conversations (recovery of the "project-scoped memory" row)
A private chat can be about a project. The request carries an optional `project` (a folder name, 1 to 80 characters, no control or direction characters, no path separators); the service offers the notes that apply everywhere plus that project's notes, never another project's. Without a project only the everywhere notes are offered, as before. A kept conversation stores its project: a message without one keeps it, a message naming another moves the conversation there, an empty name clears it. Stores made before this gain the `sessions.project` column on open, rows intact.

- Terminal: `/project` shows it, `/project NAME` sets it, `/project none` clears it. On start it is the folder of the nearest repository above the working folder, and none outside a repository. The status line and each reply line say which project was used.
- Web: a "Project (optional)" field on the chat screen with an explanation; a kept conversation restores it.
- Source: Anamnesis keeps a four-way label per record and cannot separate two projects; OpenCode binds each session to a project found from the directory. See the trace row for the reading depth.
- Tightened while doing this: a project name may hold no control character at all. Before, the shared text check let a tab or a line break through, which would break the inline display of a note's project.
- Not done: project identity is still the folder name; two clones with the same folder name share notes. A repository-derived id (as OpenCode does) would fix it and needs a migration of existing notes. MCP exposes no notes; Telegram chat has no project.

## Near-duplicates (recovery of the "memory de-duplication" row)
- **Suggestions.** A suggestion from a review is dropped when it says almost what a saved note, or another suggestion of the same review, already says. Two texts now count as the same claim only if their numbers and yes/no match AND, after lining their words up in order, every word left over is a small word or a near-identical spelling (a plural, a typo) of one left over on the other side. A word that merely moved never counts. Before this, the wording check was order-blind, so "Prefer spaces over tabs" was hidden as a duplicate of "Prefer tabs over spaces" and the owner never saw the changed preference. A synonym ("kept" for "stored") or an added content word is shown as a new suggestion: a duplicate asked twice costs a click, a hidden changed fact costs a wrong note.
- **The owner's view.** Memory screen (a collapsible section), terminal `/similar`, `GET /api/memories/similar`. Read-only: nothing is merged, forgotten or changed. The newest 200 notes the owner may see are compared and the answer says how many; each group is anchored on its newest note and holds only notes that match it directly; notes for different projects are never compared with each other; notes of fewer than four words are skipped.
- **Sources.** Anamnesis builds the report with SimHash, banded hashing, Hamming and Jaccard limits and union-find, which can chain two unrelated notes into one group and is blind to word order. Hermes de-duplicates by exact string only.
- **Found while testing: a memory backup lost notes.** The library clamped every "list notes" request to 200, and the JSON memory backup asks for everything, so an export from a store with more than 200 notes silently dropped the older ones. Fixed (me37, and sv59 exports 210 notes and restores 210 into a fresh store). The encrypted whole-store backup copies the databases and was not affected. NOT VERIFIED: whether any earlier JSON export from a real store was truncated; check any backup file you made before this fix for its note count.


## Recall quality on a published benchmark (LongMemEval, measured)

- **Why.** The merge of the three rankings was tuned on toy examples. A published retrieval benchmark can say whether it finds the right memory among thousands of chat turns, and whether the shipped numbers (meaning may add 5 notes, equal weight) are good.
- **Source (VERIFIED by reading).** LongMemEval, Di Wu et al., MIT, commit 9e0b455: `README`, `src/retrieval/run_retrieval.py`, `eval_utils.py`, `print_retrieval_metrics.py` read in full for the corpus rule and the metrics. Data: the cleaned `longmemeval_s` set (dataset card licence MIT), downloaded to `target/recall-bench/` and never committed. LoCoMo was rejected: CC BY-NC 4.0 is non-commercial.
- **Protocol (copied from the reference).** Corpus is the user turns of the history sessions; the query is the question; the right answer is the turns marked `has_answer` in the answer sessions; abstention questions and questions with no such turn are skipped; session-level recall (any, all) at 5 and 10, NDCG at 10 with the reference formula, and turn-level recall. Ports of the metrics are in `crates/kk-cli/src/recall_bench.rs` (ignored test `recall_benchmark`, run with `KK_RECALL_SAMPLE` pointing at a sample made by `scripts/recall-bench-prepare.py`).
- **Sample.** 275 questions chosen by a fixed content-hash order, equal numbers per question type (60 each; 30 preference, 5 assistant), 67,198 user turns. 105 questions are for choosing settings, 170 are held out and only reported. Real embedding model: `nomic-embed-text` through Ollama. The store refused 182 turns (too long or empty after trimming); none of them held evidence.
- **Setting choice.** 61 settings (meaning cap 5/10/20/50, closeness 0.35 to 0.60, meaning weight 1/2/4) were scored on the 105 development questions only. The shipped setting was kept unless another beat it by 0.01 on session all@10, then any@5, then NDCG@10.
- **Result on the 170 held-out questions (session level, higher is better).**

| | any@5 | all@5 | any@10 | all@10 | NDCG@10 | turn all@10 |
|---|---|---|---|---|---|---|
| keywords and similar wording only | .935 | .782 | .947 | .876 | .520 | .718 |
| as shipped before (cap 5, weight 1) | .976 | .876 | .994 | .929 | .593 | .806 |
| chosen (cap 10, weight 4, closeness .5) | .988 | .924 | .994 | .947 | .642 | .859 |
| plain meaning search, no merge | .988 | .918 | .988 | .959 | .680 | .865 |

- **Read honestly.** Meaning clearly helps over keywords (most on preference and multi-session questions). The chosen setting beats the old one on every aggregate number; the largest gain is multi-session questions (all@5 .650 to .825, 40 questions). One preference question out of 10 moved from found to not found at the top 5 (it is found by rank 10). Plain meaning search still has a better NDCG than our merge (.680 against .642), so the merge is not yet best; what the keyword rankings add is exact-term recall on questions that name a thing, which this benchmark rewards little.
- **Built.** `MEANING_ONLY_CAP` 5 to 10, new `MEANING_WEIGHT` 4.0, `Memories::set_meaning_cap`, `set_meaning_weight`, `fuse::fuse_weighted` (a missing, negative or non-finite weight counts as 1, above 10 is clamped to 10, weight 0 drops a list and the notes only it found). Tests me30, me31, fu07, fu08; eight mutants each fail a test.
- **Not measured (NOT VERIFIED).** Wrong notes shown (false positives and precision); short fact-style notes (the corpus is chat turns); other embedding models (a model with a different score spread may want another closeness); the effect on the review prompts that use recall. The benchmark is recall of the right turn, not answer quality.
