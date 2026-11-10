# Coding mode: a task in words becomes a tested patch the owner applies (ledger G-GH-32, acceptance II-09)

## Target experience
1. The complete experience: the owner picks a project folder, says what to change in plain words, watches the model read, edit and run the tests, then reads one patch with the test result beside it and applies it, or does not. Apply is exact (the patch that was shown), all or nothing, and one step undoes it. The same task works from the terminal app, the plain commands, the web page and the service.
2. Strongest references: OpenCode (edit and read tools, permission before a write), goose (tool calls recovered from a local model's text), Cline and Codex (checkpoints, patch review).
3. Approach: the model never touches the disk. It edits a copy held in memory; the tests run in the sandbox on that copy; the kernel runs them again on the final files; only the owner's approval of the shown patch writes anything. Files that hold secrets are never in the copy.
4. Quality criteria: a small local model can finish a real fix; a refused tool call tells the model what to do next; nothing withheld is ever shown, named or written; a patch that adds a secret is never applied; apply and undo survive a crash part way.

## References and reading depth
- goose (Apache-2.0, commit 71dd9a9) `crates/goose-local-inference/src/native_tool_parsing.rs`: READ IN FULL. Tool calls written as text (bare JSON, fenced JSON, `<tool_call>` tags).
- OpenCode (MIT, commit fe04656) `packages/core/src/tool/plugin/read.ts` (294 lines) and `edit.ts` (368 lines): READ IN FULL, after the first build and before the commit. `grep.ts` (151 lines): NOT READ. The code behind the `doom_loop` permission: NOT READ (only the key in `packages/core/src/v1/config/permission.ts` line 27 and its description were seen).
- Ollama (MIT, e9fde36): NOT READ; behaviour observed by running it.
- Capability matrix E rows C1 to C12 are an investigator's reading (MATRIX depth) and justify nothing here on their own.

## Decisions per capability
| Capability | Reference behaviour (read) | Decision | Where |
|---|---|---|---|
| Text tool-call recovery | goose parses JSON and tagged calls out of plain text | ADAPT, rebuilt: only names of tools offered in this turn are taken, a scan budget (64 KiB, 2,000,000 steps), at most 8 calls | `crates/kk-route/src/text_calls.rs` |
| Unique-match edit | OpenCode refuses an empty or identical old text and more than one match | Already equal (built before the reading) | `Workspace::edit` in `crates/kk-review/src/code.rs` |
| Near-copy match | OpenCode tiers: exact, typographic look-alikes, spaces at line ends; keeps the file's line endings per region | Already equal: `align_edit` (quotes, dashes, odd spaces, CRLF, line-end spaces; untouched lines keep their bytes; exactly one place or nothing), al01 to al08 | `crates/kk-review/src/align.rs` |
| Miss hint | OpenCode `missHint`: "already applied" when the new text is in the file; else lines vote for where the old text starts, half must agree, up to 12 lines shown from the first to the last matched line | ADAPT, rebuilt in Rust. Safer here by construction: OpenCode must check read permission before hinting; our workspace never holds a withheld file | `miss_hint` in `code.rs` |
| Replace every place | OpenCode `replaceAll` | ADAPT: `replace_all`, boolean only; the size cap covers the whole result | `Workspace::edit` |
| Missing path | OpenCode offers up to 3 close names | ADAPT: names that contain, or are contained in, the asked file name; only from files the model may read | `Workspace::missing` |
| Canonical-name retry (NFC, nbsp, curly quotes in a path) | OpenCode resolves a unique alternate | EXCLUDE for now: paths come from `list_files` output; no failure of this kind was observed | owed list |
| Paging advice in the read tool text | OpenCode: prefer one larger read | EXCLUDE: a file of up to 400 lines already comes back whole whatever range is asked | `Workspace::read` |
| Permission before a write | OpenCode asks per edit with a diff preview | IMPROVE: no write happens during the task at all; one approval of the whole patch, bound to its digest | `crates/kk-service/src/coding.rs` |
| Snapshot and undo | OpenCode's edit tool notes snapshots as not yet done in that file | BUILD: journaled apply and undo, settled from disk at startup; deletes only files whose bytes still match | `coding.rs`, `fixfiles::write_all_or_none` |
| Images and PDFs in read | OpenCode returns them as files | EXCLUDE: the coding copy is text only | |
| Instruction-file discovery after a read (AGENTS.md) | OpenCode attaches nearby instruction files | EXCLUDE for now: project text is untrusted here and may not steer the task; revisit with the owner-instructions component | owed list |

## Evidence
- `kk-review` tests `code.rs` cw01 to cw12 (withheld files, paths, bounded reads, unique and near-copy edits, create and delete, limits, search, the miss hint, an edit made twice, the hint's bounds, replace-all, close names), `code_loop.rs` lp01 to lp10 (the loop with a scripted model: refusals, repeats, turn limit, text calls, a patch that touches tests), `code_sandbox.rs` (6, in the VM), `align.rs` al01 to al08.
- `kk-route` `text_calls` (4). `kk-service` flow sv172 to sv177 (task, approval by digest, apply, undo, crash recovery, a patch with a secret refused). `kk-tui` tu_code01, tu_code02. Web: 192 unit tests and `tests/e2e/code_real.mjs` (8 checks in a real browser with a real local model; run again after the tools changed, 8 of 8).
- Mutants caught for the tools added after the OpenCode reading: withheld names offered as suggestions; no cap on the hint; no half rule; replace-all changing one place; a short new text counted as already applied; the hint starting at the padded start; the hint ending one line late. 7 of 7.
- Hostile cases held: a withheld file's name is never suggested for any probe; the whole file sent back as old text shows 12 lines; invented lines around true ones do not widen the view; forty guessed lines with one true line give no hint; a 200,000-line old text answers in time.

## Measured with real models (OBSERVED, this Mac, Ollama)
- qwen2.5-coder:7b finished the two-bug pricing task in the web page (tests fail, two edits, tests pass, apply, undo).
- qwen2.5:3b did not: it narrates instead of calling tools and loops on reads. The task then ends with its reason in words ("the model kept repeating the same call and was stopped", "used every turn"), not with a patch.
- One task and one model family is not a quality measurement. Task success across a set of tasks and models is NOT VERIFIED.

## Limits
- Test runners: Python (pytest) and Node only. In any other project the patch comes without a test verdict and the page says "No tests were found that can run here, so nothing was checked by running it."
- The verdict is the kernel's own run on the final files, never the model's report. Found while writing this record and fixed: a model could make that run pass by weakening, deleting or adding tests, and nothing beside the verdict said so. Now a patch that changes, adds or deletes a file the runner treats as a test carries a warning for that file on every surface (`is_test_file`, `concerns`; lp10; 3 mutants caught). The verdict line itself still does not count old tests apart from new ones (owed).
- Local route only. A cloud route for coding waits for NEAR AI Cloud (BLOCKED on credits).
- The hint and the close-name suggestion were not measured for how much they raise a small model's success rate.

## Owed
1. Read OpenCode `grep.ts` and the repeated-call (doom loop) code; compare with `search_text` and `SAME_CALL_LIMIT`.
2. A task set (10 or more real fixes, 3 runs each, worst reported) for the 7b model, with and without the miss hint.
3. More runners (Rust, Go) through the sandbox; a verdict line that runs the tests as they were before the task as well as after.
4. Canonical-name retry for a path, if a real failure shows it is needed.
5. Text tool-call recovery in the other agent loops (chat tools, research), decided per loop.
