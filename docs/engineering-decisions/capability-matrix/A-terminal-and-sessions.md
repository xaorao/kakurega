# Capability matrix A: terminal and sessions

Scope: the `kaku` full-screen terminal workspace and the session model behind it. Kakurega state is read at commit 8eb997c. Labels: VERIFIED (read in code or run), OBSERVED (seen in a file listing, test name or doc), INFERRED (reasoned from what was read), UNKNOWN (not read). No dates are recorded.

Fixed constraints applied to every decision below:
- One kernel, one approval path. A terminal feature may present or request, never decide. Release approval stays `/admin/approvals/{id}/approve` with the typed digest prefix and the signed proof (`crates/kk-tui/src/app.rs` 326-366).
- Remembered permissions, auto-approve flags and headless approval are excluded for anything that releases data. Local read-only convenience, if ever added, belongs to the policy engine, not the screen.
- File and folder references are resolved by the service so labels and taint apply. The client never reads a file and pastes it into a model request.
- Conversation text is private data. Anything persisted goes through the service's encrypted vault storage, never plain files in the user's home.
- Permissive code (MIT, Apache-2.0) may be adapted with a NOTICE.md entry and no licence comments in code. Everything here is MIT or Apache-2.0, so adaptation is legal. Most of it is TypeScript, so in practice it is a port of an algorithm with credit.

## 1. Sources

| Reference | Commit | Licence | Files read (lines) | Depth | Not read |
|---|---|---|---|---|---|
| Kakurega terminal | 8eb997c | project | `crates/kk-tui/src/cmd.rs` 1-204; `run.rs` 1-99; `http.rs` 1-117; `ui.rs` 1-288; `app.rs` 1-1159 (the handlers for Deny, Deliver, MemoryList, Remember, Correct, Fixes, ApplyFix, Forget, Ask, Chat at 1160-1532 only grep-located); `crates/kk-cli/src/tui.rs` 1-79; `docs/engineering-decisions/terminal-workspace.md` 1-25; `crates/kk-service/src/service.rs` 971-1011 (chat handler) and 2405-2470 (routes); `crates/kk-cli/src/main.rs` 20-70 | READ IN FULL for cmd, run, http, ui, tui.rs, record; PARTLY READ for app.rs | `crates/kk-tui/tests/app.rs` (only test names TU01-TU18 listed, bodies not read); `kk-vault` and `vault-storage.md` (so the session storage fit is UNKNOWN) |
| OpenCode | fe04656 | MIT | `packages/tui/src/prompt/history.tsx`, `stash.tsx`, `frecency.tsx`, `mention.ts`, `component/prompt/draft-stash.ts` (all in full); `component/prompt/autocomplete.tsx` 1-520 of 1023; `component/prompt/index.tsx` 440-460, 498-548, 1099-1130, 1405-1470 plus a grep of all 2036 lines; `routes/session/permission.tsx` first ~330 non-import lines of 622; `routes/session/dialog-fork.tsx`, `dialog-message.tsx` (full); `component/dialog-session-list.tsx` first ~200 non-import lines of 327; `feature-plugins/system/notifications.ts` (full); `attention.ts` 1-80; `feature-plugins/prompt/btw.tsx` 1-60, `goal.tsx` 1-50; `mini/verbosity.ts` 1-40, `mini/runtime.queue.ts` 1-60, `mini/scrollback.surface.ts` 1-50; `routes/session/history.ts` 1-40, `rows.ts` 1-30; `test/session-backfill-disclosure.test.tsx` 1-50; `config/keybind.ts` 1-60; `core/src/session/inbox.ts` 1-150 of 625; `core/src/session/revert.ts` (full 112); `core/src/permission.ts` 1-140 of 388; `core/src/permission/saved.ts` 1-60; `schema/src/session-inbox.ts` 1-20 plus grep; `cli/src/commands/commands.ts` about 395-620 | PARTLY READ | `session-tabs.tsx` (1790), `form.tsx`, `diff-viewer.tsx`, `compaction.ts`, `transfer.ts`, keymap, theme, plugin system, editor-context, `cli/src/run/noninteractive.ts`, all of `packages/server` |
| Hermes Agent | 29a61cb7 | MIT | `ui-tui/README.md` (full 515); `tui_gateway/AGENTS.md` (full 207); `ui-tui/src/app/useSubmission.ts` 1-140 and grep-located 270-300, 440-445, 524; `app/gatewayRecovery.ts` (full 35); `lib/history.ts` (full 82); `tui_gateway/methods_session_branch.py` 1-120 of 168; `checkpoints.py` 1-40; `session_reaper.py`, `session_auto_continue.py`, `notification_park.py`, `event_replay.py`, `session_registry.py`, `methods_session_interrupt.py` each 1-35; `hermes_state_search.py` 1-60 | PARTLY READ (the two docs are full, the code is sampled) | `turnController.ts`, `useQueue.ts`, `textInput.tsx`, `markdown.tsx`, `useVirtualHistory.ts`, the Ink fork, `session_history.py`, the other 35k lines of the gateway |
| Codex | f2b94ef (named in the task) | Apache-2.0 | The clone at f2b94ef contains only `codex-rs/linux-sandbox`; `codex-rs/tui` and `app-server` are NOT present there (VERIFIED by listing). The sparse clone `private-ai/inspiration/codex-tui-src` at 021fccb holds `codex-rs/tui` (1198 `.rs` files), and that is what was read: `slash_command.rs` 1-200 of 378; `session_queue_commands.rs` 1-120 of 203; `screen_reader.rs` 1-90 of 193; `session_lock.rs` (full 45); `pager_overlay.rs` 1-60; `app_backtrack.rs` 1-60 of 1243; `bottom_pane/paste_burst.rs` 1-45; `bottom_pane/chat_composer_history.rs` 1-70; `bottom_pane/pending_input_preview.rs` 1-45; `bottom_pane/approval_overlay.rs` 1-80 plus grep of 2598; `resume_picker.rs` doc comments and signatures by grep; `notifications/mod.rs` 1-40; `insert_history.rs` doc header; `tui.rs` grep | PARTLY READ (mostly module headers and doc comments) | The composer body, textarea, run loop `lib.rs`, keymap.rs (4439), markdown renderer, history cells, streaming controller; all of `app-server` (absent) |
| Cline CLI | e68a6dd | Apache-2.0 | `apps/cli/README.md` 1-200; `src/tui/commands/slash-command-registry.ts` 1-80; `src/tui/checkpoint-picker-items.ts` 1-70; `src/tui/components/queued-prompts.tsx` 1-50; `src/cli.tuistory.e2e.test.ts` 1-60; `tool-approval.tsx` by grep; directory listings of `src/commands`, `src/tui` | PARTLY READ | `main.ts`, `runtime/*`, connectors, hub daemon, schedule, doctor body |
| Goose | 71dd9a9 | Apache-2.0 | `crates/goose-cli/src/session/input.rs` 1-140 of 887; `session/mod.rs` 1400-1445, 1718-1760, 2297-2350 plus grep of 3444; `session/program_status.rs` 1-50; `session/paste.rs` doc 1-40; `session/streaming_buffer.rs` doc 1-60; function lists of `commands/session.rs` and `session/editor.rs` | PARTLY READ | `completion.rs`, `output.rs`, `builder.rs`, `elicitation.rs`, the rest of `streaming_buffer.rs` (1033 lines), `goose` core |
| OpenClaw | f38495f7d | MIT | `src/tui/tui-plugin-approvals.ts` 1-60 of 395; `tui-last-session.contract.ts` (full 19); `tui-snapshot-journal.ts` (full 33); `embedded-session-steering.ts` 1-35; directory listing of `src/tui` | PARTLY READ | everything else in the 60-file TUI |
| Cline hub, Privateer | e68a6dd / 181a988 | Apache-2.0 / MIT | Privateer `bin/privateer-tui` head only: a launcher that execs Pi's TUI, so there is no TUI of its own to study | NOT READ | Privateer TUI (Pi), Cline hub daemon |

Other strong terminal clients exist that are not on disk and were not read: Gemini CLI, Aider, Crush, Claude Code (closed, docs only), Amp. Their licences are NOT VERIFIED here. OpenClaw was added because it is MIT, has a 60-file TUI with session picker, steering and approval controllers, and shares Kakurega's need for approvals under concurrency. Privateer was checked because it competes in the same track; it adds nothing here.

## 2. What Kakurega has today (VERIFIED unless stated)

- Alt-screen ratatui app. A pure `App::handle(Event) -> Vec<Req>` reducer, a worker thread that runs HTTP calls, a 700 ms tick thread (`run.rs` 63-71), and a draw on every loop iteration at a 60 ms poll (`run.rs` 75-81).
- 20 slash commands (`cmd.rs` 25-29). Tab completes only the command name, with unique-prefix and common-prefix logic (`cmd.rs` 173-204). No argument completion and no popup.
- Single-line composer with Emacs-style editing, bracketed paste flattened to spaces, in-memory history of 200 (`app.rs` 439-570).
- Plain text is a private chat via `POST /api/chat` with the last 18 turns held on the client (`app.rs` 718-737). The service call is request/response, not streaming (`service.rs` 971-1011). One in-flight message at a time ("Still waiting for the last answer").
- Conversations are never saved ("/clear ... never saved", `app.rs` 636).
- Review progress by polling `GET /api/jobs/{id}` every 700 ms per running job; each reply carries the full event list and the client skips what it already printed (`app.rs` 959-1017).
- Approval view: scrollable release text, typed 12-character digest prefix, Esc or Ctrl-C records a refusal, signed proof when the service requires it (`app.rs` 322-382, 426-437, `ui.rs` 235-287). Paste into the digest field is accepted and filtered to hex (`app.rs` 270-273).
- Hostile-text defence: control and bidi characters replaced (`app.rs` 136-148); `NO_COLOR` honoured; every card kind has a symbol (`ui.rs` 92-99).
- Terminal restore on exit and panic (`run.rs` 16-41).
- Transcript is an unbounded `Vec<Card>` that is re-wrapped in full on every draw (`app.rs` 215-222; `ui.rs` 153-157).
- No service route to cancel a job: `grep -n cancel crates/kk-service/src/service.rs` finds nothing. Routes that exist and matter here: `/api/fs`, `/api/jobs/{id}/files`, `/audit`, `/findings`, `/fixes`, `/release`, `/api/memories/*` including history, export and import, `/api/log/export` (`service.rs` 2405-2470).
- `kaku` with no arguments opens the TUI and auto-starts `kaku serve` as a child that exits with the parent (`main.rs` 46-49; `crates/kk-cli/src/tui.rs` 22-71).
- Gap found: `terminal-workspace.md` line 21-22 promises `crates/kk-cli/tests/tui.rs` and "a real pseudo-terminal run". That file does not exist (`ls crates/kk-cli/tests` shows `mcp.rs`, `tls.rs`, `mcp_tools.golden.json`) and no pty crate is referenced anywhere in `crates` (grep for openpty, portable_pty, rexpect, forkpty is empty). The leave-the-terminal-clean claim is therefore NOT VERIFIED by any test I can find.

## 3. Capability matrix

Format per row: what the reference does, then how, then Kakurega state, decision, value, acceptance idea.

### A01 Persistent sessions: save, list, resume
- Reference: OpenCode stores sessions in a SQLite database with an inbox of pending inputs and a message table (`core/src/session/inbox.ts` 1-150, `store.ts` listed not read). Hermes keeps a `state.db` with full-text search, incremental flush every interval and on SIGTERM so a SIGKILL loses at most one interval (`session_reaper.py` 1-35, `hermes_state_search.py` 1-60). Codex lists threads over its app-server with cursor pagination (`resume_picker.rs` doc comment, lines 490-506). Cline and Goose have `history`, `session list/export/import/remove/rename` (`commands/session.rs` function list; `cli/src/commands/commands.ts` session subcommands).
- Kakurega: LACKS. Conversations are never saved by design (`app.rs` 636). Reviews are saved by the service as jobs and the TUI relists them on start (`app.rs` 889-934), so review resume is PARTIAL, chat resume does not exist.
- Decision: BUILD, with a hard privacy shape. A `sessions` store inside the service, in the encrypted vault, off by default or explicit per session ("keep this conversation"), with a forget that deletes text and leaves a tombstone like memories do. The terminal only calls `/api/sessions`. Do not copy the plain SQLite and plain JSONL files the references use. Whether `kk-vault` can host this is UNKNOWN (not read).
- Value: high.
- Test: start `kaku`, chat three turns with keep on, kill the process with SIGKILL, restart, `/resume` shows the session with all three turns; with keep off nothing is listed; `/forget-session` leaves no plaintext in the data directory (grep the vault files for a canary string).

### A02 Session picker
- Reference: OpenCode `component/dialog-session-list.tsx` (first ~200 non-import lines): debounced server search (150 ms), project-versus-everything scope toggle, pinned group, date groups ("Today", date), attention gutter showing running or needs-input, quick-switch slots 1-9, delete needs a second press to confirm, degraded search on transport failure instead of crashing. Codex `resume_picker.rs`: compact multi-line records, Ctrl+E expands a lazy transcript preview, toolbar focus with Tab, cursor pagination with de-duplication across pages, filtering at backend plus typed filter. Hermes: picker with 1-9 quick pick (README 200-206).
- Kakurega: LACKS (`/reviews` prints a flat card list, `app.rs` 889-934).
- Decision: ADAPT the behaviour, not the code (TS and a different framework). Build a list-selection overlay widget in `kk-tui` reused by sessions, reviews, memories and the future command popup. Keep: debounced search, relative dates, preview on demand, double-press delete, error shown in the list. Add: label showing whether a session holds private data classes.
- Value: high.
- Test: 200 seeded sessions; type a query, only matches remain within one frame budget; kill the service mid-search, the picker shows "could not search" and stays usable; delete needs two presses.

### A03 Resume at launch
- Reference: OpenCode `--continue`, `--session`, `--fork` on `run` and the root command (`commands.ts` 400-470). Codex `named_session_lookup.rs` (listed) and `codex queue` resolve a session by id or name. OpenClaw persists the last session key per scope (`tui-last-session.contract.ts`).
- Kakurega: LACKS.
- Decision: BUILD `kaku --continue` and `kaku --session NAME`, depends on A01.
- Value: medium.
- Test: `kaku --continue` after a kept session opens it and the status line names it; with none kept it opens a fresh one and says so.

### A04 Fork or branch a session
- Reference: OpenCode `dialog-fork.tsx` (full) forks the full session or before a chosen user message and restores that prompt to the composer; Hermes `methods_session_branch.py` 1-120 copies only visible user and assistant rows, names the child by lineage, and takes an idempotency key so a retry returns the same child (and "nothing to branch" error when empty); Codex `/fork` and `/side` (an ephemeral fork that is thrown away, `slash_command.rs` description at line ~146); Cline `/fork`.
- Kakurega: LACKS.
- Decision: ADAPT. Fork copies history into a new vault session with a parent pointer; the idempotency key is worth taking because the terminal talks over a flaky local HTTP. Disclosure accounting must not carry over: a fork starts with zero release history unless the kernel says otherwise (INFERRED requirement; the cumulative disclosure belongs to the job, not the chat).
- Value: medium.
- Test: fork before turn 2, the child holds turn 1 only; send the fork request twice with one key, one child exists.

### A05 Rewind to an earlier prompt, with file revert
- Reference: Codex Esc-Esc backtrack: first Esc primes, second Esc browses user prompts, Enter reverts before the chosen turn and puts the prompt back into the composer (`app_backtrack.rs` 1-25). OpenCode `revert.ts` (full): stage, clear (undo the revert) and commit, restoring files from snapshots recorded per assistant message, with a warning-free path when files are not wanted. Cline `/undo` checkpoint picker; its comment in `checkpoint-picker-items.ts` 36-46 records a real bug: run numbers must be counted the same way core does or restore fails.
- Kakurega: PARTIAL. `/applyfix` edits the user's own file after a y/N confirm (`app.rs` 383-421), with no undo.
- Decision: ADAPT in two parts. (1) Conversation rewind: fork-in-place plus restore prompt, after A01. (2) File undo after `/applyfix`: the service snapshots the file before applying and offers `/undofix N`; stage, clear, commit semantics from OpenCode. This is a local edit of the user's own file, so it does not touch release approval.
- Value: high for undo of applied fixes, medium for chat rewind.
- Test: apply a fix, `/undofix`, file bytes equal the original (hash compare); undo twice is refused with a clear message.

### A06 Message queue while busy
- Reference: Hermes: plain text typed while busy is queued; slash commands and `!cmd` never queue; queue auto-drains after each reply; Up/Down edits queued items before history; an edited item moves to the front (README 205-215). Codex `pending_input_preview.rs` (1-45) renders pending steers, rejected steers resubmitted at end of turn, then queued messages, with an edit key. OpenCode has an explicit `prompt.queue` command and a durable server-side queue (`inbox.ts`, `mini/runtime.queue.ts` 1-60 says queued prompts are admitted to the server's durable queue rather than staying editable client state). Cline `queued-prompts.tsx`: Enter steers, Tab edits, Esc cancels the turn. Codex also has a headless `codex queue TARGET MESSAGE` with a client message id (`session_queue_commands.rs` 1-120).
- Kakurega: LACKS. A second message is refused with "Still waiting" (`app.rs` 719-722).
- Decision: BUILD on the client first: a bounded queue of raw text, preview strip above the composer, edit-in-place, drain on completion. Rule taken from Hermes and Codex: only plain chat text queues. Commands with effects (`/approve`, `/deliver`, `/applyfix`, `/forget`, `/release`) never queue, because a queued approval is a deferred release decision that the owner is no longer looking at. Stay with a client queue (not OpenCode's durable server queue) because durable queued text is private data at rest.
- Value: high.
- Test: send three messages while the first is pending; they run in order; `/approve` typed while busy runs immediately or is refused, never queued; kill the service while two are queued, nothing is silently lost (the preview stays and shows an error).

### A07 Steer and interrupt, busy-input modes
- Reference: Hermes has three busy modes, queue, steer, interrupt (default interrupt), chosen per session, with `/busy` to switch; steer injects at the next tool boundary and falls back to queue if rejected (`useSubmission.ts` 270-300, `interfaces.ts` BusyInputMode). OpenCode: `delivery` is `steer` or `queue`; steers are promoted at a step boundary, queued input only when idle (`inbox.ts` doc comment on `Promotable`); Esc pressed twice within 5 s interrupts and resumes (`prompt/index.tsx` 498-535, INFERRED from the counter reset timer and `>= 2` check). Goose Ctrl-C cancels the run via a cancellation token; Ctrl-C with text only clears the line; a second Ctrl-C on an empty line exits (`input.rs` 55-85).
- Kakurega: LACKS. Ctrl-C clears input or quits; it cannot stop a chat request or a review (`app.rs` 443-450). There is no job-cancel route.
- Decision: BUILD interrupt first, steer later. (1) Esc during a pending chat or ask aborts the request client-side and tells the service to drop it. (2) A `POST /api/jobs/{id}/cancel` route and `/stop [JOB]`, which must leave no half-prepared release approvable (a cancelled job's pending approval is denied, not left open; INFERRED requirement). (3) Double-press guard for Ctrl-C quit while work is running. Steering a running review mid-flight is EXCLUDE for now: the review worker's inputs are the kernel's business and a mid-run instruction is an unlabelled input; reconsider only if the worker loop exposes a labelled steering channel.
- Value: high (cancel), low (steer).
- Test: start a review, `/stop`, state becomes cancelled, `/approve` says nothing to approve, event log shows the cancel; Esc during a slow chat returns control within 500 ms and a late reply is discarded, not shown.

### A08 Interrupt hygiene: never leave the conversation invalid
- Reference: Goose `handle_interrupted_messages` (`session/mod.rs` 1718-1760) answers every dangling tool request in the last assistant message with an explicit error ("Interrupted by the user to make a correction") so the transcript stays valid; on a cancelled permission prompt it drains the stream and adds the missing response (1400-1445).
- Kakurega: INFERRED NA today (chat has no tool calls), becomes relevant with streaming and the worker loop.
- Decision: IMPROVE when A07 lands: a cancelled turn is recorded as a visible "cancelled" card plus a session entry, so a resumed session never contains a half answer presented as complete.
- Value: medium.
- Test: cancel mid-stream, resume the session, the partial text is marked cancelled and is not sent as history to the model (or is sent clearly marked).

### A09 Multiple concurrent jobs and agents: roster, dock, detail, tail
- Reference: Hermes live-agent dock above the composer, Ctrl+T roster, Enter for details, `t` for a bounded live tail (last 16 KiB), `e` steer, `x`/`X` stop child or subtree (README 70-85; `AGENTS.md` "Shared subagent snapshots" says `subagent.tail` returns the last 16 KiB and control is pinned to the exact session and generation). OpenCode: session tabs with attention state, `subagents-tab.tsx` (listed, not read). Codex `/agents`, `/subagents`, `/ps`, `/stop` (`slash_command.rs`). Cline agent teams.
- Kakurega: PARTIAL. The status line counts running reviews (`app.rs` 242-264) and each progress event prints once as a card. There is no roster, no detail view, no tail.
- Decision: ADAPT the dock and the detail view over the existing job events; EXCLUDE steer and subtree stop. Show per job: state, recipient, last stage, elapsed, and whether it waits for approval. Also show the number of files and the cumulative disclosure so the owner sees what is about to leave without opening `/release`.
- Value: medium.
- Test: three reviews running; dock shows three rows on a 24-line terminal without covering the composer; the row for a job awaiting approval is visually distinct without colour.

### A10 Background work that finishes while you are elsewhere
- Reference: Hermes parks notifications for sessions that are not live and delivers them on resume, with per-owner and total caps and progress notices yielding to results (`notification_park.py` 1-30). OpenCode notification rules: one "done" per execution, no done while a child session still runs, errors suppress done, subagents are silent unless a sound is wanted (`feature-plugins/system/notifications.ts`, full).
- Kakurega: PARTIAL. Jobs live in the service so a restarted TUI re-lists them and re-watches running ones (`app.rs` 925-933), but a job that finished while the TUI was closed produces no "finished" card.
- Decision: ADAPT the idea cheaply: on start, compare job states with the last-seen marker kept by the service and print "2 reviews finished while you were away". No parking queue is needed because job state is already durable.
- Value: medium.
- Test: start a review, quit the TUI, wait for it to finish, reopen, a card says it finished and what to do next.

### A11 Notifications and terminal status
- Reference: Codex chooses OSC 9, OSC 777 or BEL from terminal detection (`notifications/mod.rs` 1-40). OpenCode raises a desktop notification only when the terminal is blurred and plays distinct sounds for question, permission, error, done; text is stripped of ANSI and control characters and limited to 80 and 240 characters (`attention.ts` 1-80). Goose emits a bell on a permission prompt and reports a program status (working, blocked on permission, done, error) to the terminal with a base64 message capped at 2048 bytes (`program_status.rs` 1-50, `mod.rs` 2297-2300); the exact escape sequence was not read.
- Kakurega: LACKS. There is a Telegram bridge with counts and never contents (CLAUDE.md, `kaku telegram`); the terminal rings nothing.
- Decision: ADAPT. Notify on three events only: approval waiting, review finished, service error. The text is fixed and generic ("Kakurega: a release needs your approval"), never a recipient name, filename or finding title, because a notification is shown by other software (the desktop, a tmux status bar, a screen share). Honour `NO_COLOR`-style opt-out (`KAKU_NOTIFY=off`). Use BEL as the safe default, OSC 9/777 where detection is certain.
- Value: medium.
- Test: approval ready while the terminal is not focused emits exactly one notification whose text contains none of the job's data (assert on the bytes written to the pty).

### A12 Permission prompt design
- Reference: OpenCode: once, always (shows the exact rule that will be saved), reject; reject opens a free-text box "tell it what to do differently" so the refusal carries feedback; Esc maps to reject; edit requests render a split or unified diff by terminal width (`permission.tsx`, first ~330 non-import lines). Hermes: `o`, `s`, `a`, `d` quick picks, Esc and Ctrl-C deny, and a withdrawn question arrives as `request.cancel` and clears the prompt (README 190-215; `AGENTS.md` server_requests). Codex: explicit decision event on every selection; Esc is always Cancel for elicitations even with custom keybindings; queue of requests; externally resolved requests dismiss the overlay (`approval_overlay.rs` 1-12, plus the prior record). Cline: `y` or Enter approves, `n` or Esc rejects (`tool-approval.tsx` 143-147). Goose: Allow, Always Allow, Deny, Cancel; headless mode refuses to run in approve modes rather than auto-allow (`session/mod.rs` 1400-1445).
- Kakurega: HAS, and stronger than all of them for release approval: typed digest prefix, refusal recorded, signed proof. Weaker spots: (a) paste is accepted into the digest field (`app.rs` 270-273); (b) no refuse-with-reason; (c) no handling of an approval resolved elsewhere (the web app, `kaku approve`) while the overlay is open: the next Enter would POST and fail. INFERRED, not tested.
- Decision: IMPROVE, do not import any "always" option. Add (1) optional reason on refusal, written to the signed log; (2) poll or subscribe for the approval's state while the overlay is open and dismiss with a card if it is resolved or expired; (3) keep Esc as refusal and make it non-remappable (Codex rule); (4) decide deliberately about paste: either require key events for the digest or keep paste but record in the approval statement that the digest was pasted. I recommend requiring key events, since pasting turns the friction into a clipboard formality; this is a judgement call for the owner, not a verified defect, because the signed proof is the binding.
- Value: high.
- Test: open an approval, approve it from the web app, the terminal overlay closes with "already approved elsewhere"; refuse with reason, the log entry carries the reason; paste of 12 hex characters is handled per the chosen rule.

### A13 Remembered permissions and rule languages
- Reference: OpenCode evaluates ordered wildcard rules with last match winning, default ask; saved permissions are per project, listable, editable and removable (`core/src/permission.ts` 1-140, `permission/saved.ts` 1-60); an `Unattended` ruleset turns would-ask into deny for work nobody can answer (permission.ts 44-49). Codex offers "don't ask again for commands starting with PREFIX" and per-session grants (`approval_overlay.rs` 870-910).
- Kakurega: policy lives in the policy engine (docs `policy-engine.md`, not read here). The terminal has no permission rules of its own, correctly.
- Decision: EXCLUDE remember-always for release approval. IMPROVE only by borrowing the Unattended idea at the policy layer: any context with no human at a keyboard (headless, scheduled, Telegram-triggered) resolves "ask" to "deny". Record as a requirement for the policy engine owner, not a terminal feature.
- Value: high as a policy invariant, none as a terminal feature.
- Test: a headless run that reaches an approval step ends with a denied state and a log entry, never an approval.

### A14 Approval requests under concurrency
- Reference: OpenClaw `tui-snapshot-journal.ts` (33 lines): when a pending-approvals snapshot is fetched asynchronously, events that arrive meanwhile are journaled and replayed over the snapshot so an approval is neither lost nor resurrected; `tui-plugin-approvals.ts` strips bidi controls and flattens whitespace before display and coalesces refreshes. Hermes `event_replay.py`: per-session sequence numbers, a bounded ring, and an epoch so a client detects a server restart.
- Kakurega: PARTIAL. One approval at a time per command; no list of pending approvals; two jobs ready means two separate `/approve` runs.
- Decision: ADAPT the journal pattern (MIT, 33 lines, trivial to port) for an approvals queue shown in the status line ("2 waiting"). Each still opens its own full overlay, never a batch approve.
- Value: medium.
- Test: two jobs ready; approve one in the web app while the terminal refreshes; the list never shows a stale entry and never loses the other.

### A15 Slash command registry
- Reference: Codex: an enum whose order is the popup order, per-command description, aliases, `supports_inline_args` (`slash_command.rs` 1-200). Cline: a registry entry has source (tui, runtime, plugin, skill, workflow), execution kind, visibility, `preserveInput` (`slash-command-registry.ts` 1-80). Hermes: client commands first, everything else falls through to the server, which owns aliases, plugins, skills (README 270-280; `AGENTS.md` "Slash command flow").
- Kakurega: PARTIAL. A hand-written `match` in `cmd.rs` 66-171, a separate `NAMES` list, usage strings in a third place, and a help text in a fourth (`app.rs` 608-627). They can drift.
- Decision: IMPROVE (one static table: name, aliases, one-line description, argument grammar, effect class: read, local-write, or kernel-effect, `queueable` flag). Help, popup, completion, usage errors and the queue rule (A06) all read from it. Keep commands in the client; the service does not own slash commands, because its API is the only server interface.
- Value: high.
- Test: a unit test fails if a command exists in the parser but not in the table; every command with effect class kernel-effect has queueable false.

### A16 Completion popup and argument completion
- Reference: OpenCode popup with descriptions, fuzzy matching, modes for command, reference and directory, with `ctrl+p` palette (`autocomplete.tsx` 1-520). Hermes: 60 ms debounce, `complete.slash` for `/` and `complete.path` for tokens starting `./ ../ ~/ / @` (README 218). Cline `use-autocomplete.ts` and `command-palette.tsx` (listed). Codex `command_popup.rs`, `file_search_popup.rs`, `skill_popup.rs` (listed).
- Kakurega: PARTIAL. Name completion on Tab only, no visible list (`cmd.rs` 173-204).
- Decision: BUILD a popup rendered over the composer, driven by the A15 table. Argument completion sources: job ids from the job list, memory ids from `/api/memories`, folders from `/api/fs`. Fuzzy match with a small scorer; no new dependency is needed (Hermes ships a lightweight subsequence scorer, `lib/fuzzy.ts` listed but the path was not found at the stated location, so NOT READ).
- Value: high.
- Test: type `/ap`, the popup lists `/approve` and `/applyfix` with descriptions in order; Down then Enter inserts the command; `/approve ` lists only jobs in state ready.

### A17 @-mentions and file references
- Reference: OpenCode: `@` opens fuzzy file search (server `file.find`, limit 20), supports `path#10-20` line ranges, inserts an atomic styled token (an "extmark") that deletes as one unit, ranks by frecency `frequency / (1 + age_in_days)` capped at 1000 entries, and re-aligns mention ranges after edits with a dynamic-programming matcher (`autocomplete.tsx` 120-330; `frecency.tsx`; `mention.ts`). Codex `mention_codec.rs`, `task_mentions.rs`, `/mention` (listed).
- Kakurega: LACKS. `/ask FOLDER : QUESTION` takes a folder only (`cmd.rs` 119-125).
- Decision: ADAPT, with the kernel in the loop. A mention becomes a typed reference (path plus optional line range) sent to the service, which reads the file under its own root rules, labels it, and decides whether it may reach the model. The client never inlines file text. Mentioning a path outside the configured roots is refused with the reason. Frecency data is private (it reveals what you work on), so it lives in the vault with history, not in a plain file.
- Value: high.
- Test: `@src/lib.rs#10-20` reaches the service as a reference and not as text; a path outside the roots is refused; a file the policy labels above the route's clearance is withheld and the card says so.

### A18 Path and directory completion
- Reference: Hermes `complete.path` (note in `AGENTS.md` that under a remote terminal backend it lists the session's backend, not the gateway host: a correctness trap); OpenCode `directory-completion.ts` and `directory-recents.ts` (listed, not read).
- Kakurega: LACKS. `/review` resolves relative folders against the first root (`app.rs` 641-651); `/api/fs` exists.
- Decision: BUILD on `/api/fs`, which is already the service's view of allowed roots. Respect the Hermes lesson: always complete against the service's filesystem view, never the terminal's own working directory.
- Value: medium.
- Test: `/review ./pr` offers folders under the root only; a symlink out of the root is not offered.

### A19 Multi-line composer and editing quality
- Reference: Hermes Shift+Enter or Alt+Enter, backslash-Enter fallback, word moves, `Ctrl+W/U/K` (README 150-185). Goose configurable newline key, default Ctrl+J, rejecting keys that shadow paste or interrupt (`input.rs` 85-100). OpenCode and Codex use real textarea widgets (`textarea.rs` and `chat_composer.rs` 12.9k lines per the earlier record, NOT READ); Codex has a Vim mode toggle (`/vim`).
- Kakurega: WORSE. Single line (`ui.rs` 188-204 renders one row); cursor and delete operate on Unicode scalar values, not grapheme clusters, so emoji sequences and combining marks split (`app.rs` 305-314, 489-510, INFERRED from char-based indexing); no word-wise moves.
- Decision: BUILD a multi-line text area as its own module in `kk-tui` using grapheme segmentation and width from the crates already in the tree (unicode-width is a dependency; `unicode-segmentation` is not, NOT VERIFIED whether it is already transitively present). Evaluate `tui-textarea`-style crates only after reading them; none were read here. Chat messages are capped at 4000 characters by the service (`service.rs` 972), so the composer must show the remaining count.
- Value: high.
- Test: type a family emoji, one Backspace removes the whole cluster; Shift+Enter inserts a newline; a 4001-character message shows the limit before Enter, not after the round trip.

### A20 External editor for drafts
- Reference: Hermes Ctrl+G or Alt+G writes the draft to a temp file, suspends the UI, runs `$EDITOR`, submits on clean exit (README 218-225). Goose `editor.rs` resolves the editor and can seed the file with the last messages as context (`input.rs` 100-130). Codex `external_editor.rs` (316 lines, listed). OpenCode `/editor` (`prompt/index.tsx` 551-580).
- Kakurega: LACKS.
- Decision: ADAPT. Security shape: the temp file must be mode 0600 in a private directory, removed on every exit path, and must not be created in a synced folder. Draft text is private. Do not seed it with previous messages (Goose does) unless the user asks.
- Value: medium.
- Test: Ctrl+G opens `$EDITOR`, saved text returns to the composer, the temp file is gone afterwards even if the editor is killed.

### A21 Paste handling
- Reference: OpenCode collapses pastes of 3 or more lines, or over 150 characters, into a placeholder chip that expands on demand and is restored on submit; strips ANSI first; detects image paths (`prompt/index.tsx` 1407-1470). Goose does the same on Windows only (`paste.rs`). Codex `paste_burst.rs` is a pure state machine that treats a rapid key stream as a paste on terminals without bracketed paste, so a pasted newline never submits and pasted `?` never toggles help (1-45). Hermes expands paste tokens before a slash command runs so the command receives the real text while the transcript shows the compact form (`useSubmission.ts` 58-70).
- Kakurega: PARTIAL. Bracketed paste enabled; newlines and tabs flattened to spaces; control characters replaced (`app.rs` 269-279). No chip, no burst detection, so on a terminal that lacks bracketed paste a pasted newline submits mid-paste.
- Decision: ADAPT `paste_burst.rs` (Apache-2.0, pure, ratatui/crossterm-compatible) after reading its body (only the header was read); add the chip. Windows terminals are not covered by the current record, so this is also the first step there. Never auto-attach image paths from a paste: that would read a local file by a side effect; require an explicit `@` reference (A17).
- Value: medium.
- Test: feed 500 characters across 12 lines as key events with 1 ms spacing; exactly one chip, no submit; Enter after it submits the expanded text; text sent to `/api/chat` equals the full paste.

### A22 Input history
- Reference: OpenCode keeps 50 entries as JSONL, de-duplicates the last, rewrites the file to self-heal corrupt lines, stores text plus attached parts (`history.tsx`). Hermes keeps 1000 in a flat file with timestamps (`lib/history.ts`). Codex combines persistent cross-session history with local entries in one offset space, and Ctrl+R reverse search restarts from the newest on each query edit, skips duplicate text within a search, and retries failed batch reads (`chat_composer_history.rs` 1-25). Goose uses rustyline history.
- Kakurega: PARTIAL. 200 in memory, de-duplicates the last, removes a `/remember` line that looked like a secret (`app.rs` 558-562, 739-756). No search, none persisted.
- Decision: IMPROVE. Persisted history is a leak risk: all three references write plaintext prompts to disk. Keep it in the vault, cap it, exclude commands that carry free text for memory (`/remember`, `/correct`), run the same secret check before storing any line, and give `/clear-history`. Add Ctrl+R search using the Codex semantics (restart on edit, unique matches, no hidden cursor advance at a boundary).
- Value: medium.
- Test: type a line containing a canary token that matches the secret detector, it is not in history after restart; Ctrl+R finds an old line by substring and Esc restores the draft that was being typed.

### A23 Drafts and stash
- Reference: OpenCode `draft-stash.ts` (full): one draft per tab survives remounts and is consumed on restore so a stale copy cannot shadow newer input; a separate stash of 50 with push, pop, list and remove (`stash.tsx`). Codex `startup_draft*.rs` (listed).
- Kakurega: LACKS. Starting an approval or confirm replaces nothing (composer keeps the text), so no loss there; switching sessions will lose drafts once A01 exists.
- Decision: ADAPT the consume-on-take rule when sessions arrive; the stash is optional. Drafts go through the same vault rule as history.
- Value: low to medium.
- Test: type text, switch session and back, text returns once; a second return shows nothing.

### A24 Shell escape and inline command interpolation
- Reference: Hermes `!cmd` runs a shell command and `{!cmd}` interpolates output into a prompt before sending; queued drafts keep the raw text until sent (README 190-215, `useSubmission.ts`). OpenCode has a shell mode entered with `!` and a shell tab (`prompt/index.tsx` 975-1020, `composer/shell-tab.tsx` listed).
- Kakurega: LACKS.
- Decision: EXCLUDE. A shell started from the composer, or output spliced into a model prompt, bypasses the sandbox, the broker and the labelling that the kernel exists to provide. The only shell-like escape that fits is opening `$EDITOR` (A20).
- Value: none for Kakurega (high for the references' audience).
- Test: `!ls` is sent as ordinary chat text, not executed; a unit test pins that no code path in `kk-tui` spawns a process other than the editor and the service.

### A25 Side question that does not enter the conversation
- Reference: OpenCode `/btw` runs a one-shot answer from the session context with no tools and without adding to the conversation (`btw.tsx` 1-60, instruction text at lines 1-6). Codex `/side` and `/btw` start an ephemeral fork. Hermes `/btw` (README command list).
- Kakurega: LACKS.
- Decision: ADAPT as `/aside TEXT`: runs through the same chat route with no stored history and a flag that the answer is never written to memory or session. Value is real for privacy: asking something sensitive without it persisting in a kept session.
- Value: medium.
- Test: after `/aside`, the saved session contains neither question nor answer; the service log records the call.

### A26 Goal, plan mode, todo panel
- Reference: OpenCode `/goal` stored per session via RPC (`goal.tsx` 1-50) and a todo sidebar (`feature-plugins/sidebar/todo.tsx`, listed); Codex `/plan`, `/goal`; Cline plan/act toggle; Hermes todo panel and `todo.ts`.
- Kakurega: LACKS; the worker loop is a review pipeline with stages already shown as progress events.
- Decision: EXCLUDE a generic todo panel; the review stages are the plan. Revisit only if chat gains tool use. A "plan then confirm" step for `/review` (show the folder, file count, recipient and what the kernel will do before it starts) is IMPROVE-level and belongs to the review flow, not to a plan mode.
- Value: low.
- Test: not applicable.

### A27 Diff viewing and edit flows
- Reference: OpenCode renders unified or split diff by width (split above 120 columns), with line numbers, syntax highlight, wrap, and a scroll box inside the permission prompt (`permission.tsx`, `EditBody`); a full diff viewer with a file tree (`feature-plugins/system/diff-viewer.tsx` 1380 lines, NOT READ). Hermes `inline_diff` on tool completion, `/replay-diff`. Codex `diff_render.rs`, `get_git_diff.rs`, `/diff` (listed). Cline syntax-highlighted diffs (README).
- Kakurega: PARTIAL. `/fixes` shows proposed fixes as text and `/applyfix N` confirms with y/N (`app.rs` 383-421); the Fixes handler body (1262-1310) was not read, so how a change is drawn is UNKNOWN. The release view lists files as text lines.
- Decision: BUILD a diff widget (unified, optional split above 120 columns, line numbers, `+`/`-` markers so it works without colour). Use it in `/fixes` and, importantly, in the approval overlay when a release contains modified text, so the owner reads what leaves rather than a file name. Syntax highlighting is optional; weigh a dependency (for example `syntect`) only after checking its licence and size, NOT VERIFIED here.
- Value: high.
- Test: a fix touching 3 hunks renders with correct line numbers at 80 and 140 columns; with `NO_COLOR` every added and removed line is still distinguishable.

### A28 Streaming rendering
- Reference: Goose `streaming_buffer.rs` (MarkdownBuffer): accumulates chunks and flushes only at safe points so an open `**` or code fence is never printed half-formed; large code blocks are truncated to a temp-file pointer by env-configured line limits (doc 1-60; the 1033-line body was NOT READ). OpenCode retained surfaces commit completed markdown blocks to scrollback while the tail streams (`mini/scrollback.surface.ts` 1-50). Hermes batches stream deltas and speeds them up while the user types (`useSubmission.ts` 100-130 uses `boostStreamingForTyping`, `TYPING_IDLE_MS`).
- Kakurega: LACKS. Chat is a single blocking POST with a 60 s global timeout (`http.rs` 13-19); a slow model shows nothing, then times out.
- Decision: BUILD. Service: a streaming chat endpoint (chunked or SSE) plus a cancel. Client: a `StreamingMarkdown` buffer ported from the Goose idea, tests first. One rule specific to Kakurega: text from a model is untrusted display input. It must pass the existing `clean()` before wrapping, and for `/ask`, which verifies every quote, the answer must not be displayed as verified while it streams: show "checking quotes" and render verdicts at the end. Do not stream raw model text into the log.
- Value: high.
- Test: a fake service sends `**wor` then `ld**` in two chunks; the screen never shows an unmatched `**`; a stream that stalls for 30 s shows a stalled indicator and Esc cancels; `/ask` text appears with the verdict only after verification.

### A29 Markdown and syntax rendering of model text
- Reference: Hermes small Markdown subset to Ink (headings, lists, quotes, tables, fences, inline code, links) and a diff colouring path (README 233-236). Codex `markdown_render/`, `markdown_stream.rs`, `table_detect.rs`, `terminal_hyperlinks/`; untrusted content follows ratatui's control-character filtering before trusted hyperlink escapes are added (`insert_history.rs` doc header).
- Kakurega: LACKS. Chat answers are wrapped plain text (`ui.rs` 118-127).
- Decision: BUILD a small renderer: paragraphs, lists, fenced code, inline code, emphasis, tables flattened to aligned rows. Links are shown as text with the URL visible, never as clickable OSC 8 from model output (a model can write any link text). Use `pulldown-cmark` (permissive, widely used) after verifying its licence and version in the lockfile policy; NOT VERIFIED here.
- Value: medium.
- Test: a table, a nested list and a fence render without escape sequences from the source text; a link whose text differs from its URL shows both.

### A30 Long output, collapsing, verbosity, virtual lists
- Reference: OpenCode verbosity presets quiet, default, everything, controlling thinking, tools, shell output, turn summary, footer (`mini/verbosity.ts` 1-40); tail-first history backfill when scrolling to the top (`routes/session/history.ts`, test `session-backfill-disclosure.test.tsx`); groups of parts that toggle open. Hermes details modes hidden, collapsed, expanded; virtual list with height estimation (`useVirtualHistory.ts`, 715 lines, NOT READ); ring buffer of the last 10 fan-outs. Goose truncates giant code blocks into a temp-file pointer.
- Kakurega: WORSE. Every frame re-wraps every card (`ui.rs` 153-157) and the card vector never shrinks (`app.rs` 215-222), so a long session degrades. A `/findings` with many findings, a long release list, or an `/ask` answer cannot be collapsed. INFERRED cost: O(total lines) per frame at about 16 frames per second.
- Decision: IMPROVE in two steps. (1) Cache wrapped lines per card, keyed by width, and only draw when state changes or on tick (the loop currently redraws unconditionally, `run.rs` 76-77). (2) Collapsible cards with a per-card summary line and a details toggle; verbosity levels quiet, default, everything for progress events. Temp-file pointers are EXCLUDED: writing long output to the temp directory would copy private text outside the vault; use the in-app pager (A31) instead.
- Value: high.
- Test: 5000 cards; a frame renders within a fixed budget (assert on a counter of wrap calls, not wall time); toggling a card never changes the scroll anchor on screen.

### A31 Transcript view, search, copy, export
- Reference: Codex Ctrl+T transcript overlay sharing the main viewport, with selection and search (`pager_overlay.rs` 1-60, `app_backtrack.rs` 1-25, `/copy`, `/export` markdown, `/raw` raw scrollback mode, `vim_search.rs` listed). OpenCode message actions dialog: jump, revert, copy, fork (`dialog-message.tsx`). Hermes `/copy`, `/save`, OSC 52 copy (`lib/osc52.ts`, listed). Cline `session/export.ts` (965 lines, listed). Goose `session export`.
- Kakurega: LACKS. PageUp and PageDown scroll by 8 lines; no search; no copy; no export of the transcript.
- Decision: BUILD an overlay with `/` search, jump, and a copy action that is an explicit user request. OSC 52 writes to the system clipboard, where other apps and clipboard managers can read it; label that in the confirm text and never copy automatically. Export is to a file the user names, written through the service, with the redaction rules the web app uses; one export path, not a terminal-specific one.
- Value: medium to high.
- Test: search finds a word in a 2000-line transcript and highlights the match without colour dependence; copy of a selection puts exactly those bytes into a test sink; export refuses a path outside the allowed roots.

### A32 Scrollback model: alt screen versus inline, mouse
- Reference: Codex writes finished history into the terminal's own scrollback (inline mode), keeping native selection, search and multiplexer behaviour, and uses an alt-screen only for overlays (`insert_history.rs` doc header, `tui.rs` alt-screen flag, `/raw` and `/tui` commands). OpenCode's mini mode is a direct-mode inline renderer with a footer (`mini/*`, 40 files). Goose is an inline REPL. Hermes and Cline use full-screen and offer mouse (`/mouse`).
- Kakurega: HAS alt-screen without mouse (`run.rs` 36-37). Exiting leaves nothing in the terminal's scrollback.
- Decision: keep alt-screen as the default and say why in the record. That behaviour is a privacy property: private chat and release text vanish when `kaku` exits, while inline modes leave them in the user's scrollback, tmux history and screen recordings. Add an explicit `--inline` or `/raw` mode only as an opt-in with a visible "this will stay in your terminal history" notice. Mouse wheel scroll and click-to-focus are optional; if added, restrict to scrolling to avoid capturing selection.
- Value: medium.
- Test: after quitting in default mode, the pty's scrollback buffer contains none of the typed canary strings (needs the A42 harness).

### A33 Keyboard model, keymap config, command palette
- Reference: OpenCode named commands with bindings, modes (global, autocomplete), a leader key (default Ctrl+X) and a config schema where a binding can be `false` or a list (`config/keybind.ts` 1-60); a Ctrl+P palette lists every command. Codex `keymap.rs` (4439 lines, NOT READ), `/keymap` remap UI, shortcut overlay (`shortcut_overlay.rs`, `?`). Cline command palette (`command-palette.tsx`). Hermes platform-aware hotkey strings (`content/hotkeys.ts`, listed).
- Kakurega: PARTIAL. Fixed keys, hints in the footer (`ui.rs` 218-226), no `?` overlay, no palette, no remap.
- Decision: BUILD a `?` shortcut overlay and a Ctrl+P palette over the A15 table. Remapping: allow for navigation and editing keys only; the approval keys (digest entry, Enter, Esc, Ctrl-C) and the confirm-dialog keys are compiled in and cannot be remapped, so a config file cannot weaken the approval gesture. The remap config is a plain file of key names; it must not be able to bind any key to a kernel-effect command.
- Value: medium.
- Test: config binds Esc to nothing; the approval overlay still refuses on Esc; the palette lists exactly the commands in the table.

### A34 Themes and colour
- Reference: OpenCode ships about 33 JSON themes and a system theme, theme discovery and resolution (`theme/assets/*.json`, `theme/*.ts`). Codex `styles.md` rules, `terminal_palette.rs`, theme picker. Hermes skins carried from the server (`theme.ts`). Cline `themes.ts`.
- Kakurega: PARTIAL. `NO_COLOR` only, fixed palette (`ui.rs` 14-28, 92-99); meaning never depends on colour (TU12).
- Decision: low priority; adapt one light and one dark palette that both pass a contrast check, plus a high-contrast option. Do not ship 30 themes: the value is accessibility, not choice. The design direction file (blank brand kit) governs the colours; the terminal should read the same design tokens as the web app (UNKNOWN whether tokens exist in a shareable form).
- Value: low.
- Test: each palette keeps at least 4.5:1 contrast for body text against a 16-colour and a truecolor background (computed, not eyeballed).

### A35 Accessibility
- Reference: Codex probes for a screen reader once, stores the result, and turns animations off, preserving any explicit user preference and re-reading the config after the 450 ms probe to avoid overwriting concurrent changes (`screen_reader.rs` 1-90; the detection function itself, lines 90-193, was NOT READ). OpenCode keeps semantic labels on dialogs for a simulation harness (`SimulationSemantics.bind` in `permission.tsx`).
- Kakurega: PARTIAL. No animation exists, `NO_COLOR` honoured, symbols plus words, narrow widths tested (TU11, TU12); screen-reader use was never tested (the record says so).
- Decision: IMPROVE. Add a linear mode: no overlays that redraw the whole screen, each new card announced as plain appended lines, approval as a sequential prompt. Test with VoiceOver on macOS manually, because no automated check will establish it; record that as such. A `KAKU_REDUCED_MOTION` switch is needed before any spinner is added (A28).
- Value: medium to high.
- Test: scripted run in linear mode produces an append-only byte stream with no cursor-addressing sequences except in the approval prompt; manual VoiceOver pass recorded as OBSERVED, not PASS.

### A36 Error recovery and process lifecycle
- Reference: Hermes `gatewayRecovery.ts` (35 lines, full): respawn and resume at most 3 times in a 60 s sliding window, state kept pure for testing; malformed stdout from the gateway is shown as a protocol error, never printed raw (README 18-20). Hermes writes a durable turn marker; on resume, a marker younger than 15 minutes means the turn died with the process, and the prompt is resubmitted automatically, an older one is cleared (`session_auto_continue.py` 1-35). Codex `daemon_recovery.rs`, `startup_recovery.rs`, panic tests (`tui_panic_tests.rs`). Cline `doctor` kills stray processes and opens the log.
- Kakurega: PARTIAL. Terminal restore on exit and panic (`run.rs` 16-41); transport errors become cards (`app.rs` 813-821); if the service dies, every later call errors and the TUI does not restart it (the service is started once by `tui.rs` 22-71).
- Decision: ADAPT the recovery budget (MIT, a pure function) for the child service, with a visible "service restarted" card. REJECT automatic resubmission for anything with an effect: a chat turn may be offered again with "resend?" after an interrupted turn, but a `/review`, `/applyfix`, `/deliver` or approval is never replayed automatically. The unattended deny rule of A13 applies.
- Value: high.
- Test: kill the child service during a review; the TUI shows the stop, restarts once, relists jobs, and the review is shown as interrupted with a manual action; three kills inside 60 s stop the restarts and say so.

### A37 Reconnect and event replay
- Reference: Hermes numbers events per session, keeps a bounded ring (512 events and a byte cap), returns events since a given sequence number, and carries an epoch so a restarted server is detected; truncation leaves a watermark so the client refetches instead of trusting a replay with holes (`event_replay.py` 1-35).
- Kakurega: WORSE for long reviews. Each 700 ms poll refetches the whole job object including every event (`app.rs` 285-299, 959-983), and a single slow poll is gated by `inflight_polls`. Cost grows with event count.
- Decision: IMPROVE with a `?since=N` parameter on `GET /api/jobs/{id}` (or an events sub-resource) and a server epoch. Cheap and fixes polling without needing SSE. Keep polling as the transport (see A28 for chat streaming).
- Value: medium.
- Test: 5000 events, each poll transfers only new events; a service restart changes the epoch and the client refetches from zero.

### A38 Headless and programmatic modes
- Reference: Cline: `cline "prompt"`, piped stdin, `--json` NDJSON, `--yolo`, `cline history`, schedules, ACP agent (`apps/cli/README.md` 70-160; `src/acp/`, listed). OpenCode `run` with `--continue`, `--session`, `--fork`, `--format json`, `--file`, `--title`, `--archived`, `--auto` and hidden yolo flags; `session list/export/import/delete` (`commands.ts` 400-620). Codex `codex queue` (`session_queue_commands.rs`). Goose `run`, `session list --format json`, `diagnostics`; non-interactive mode refuses to run in approve modes instead of auto-allowing (`session/mod.rs` 1400-1445).
- Kakurega: PARTIAL. `kaku verify`, `kaku log export|verify`, `kaku approve`, `kaku mcp`, `kaku telegram` exist (`main.rs` 20-45); there is no scriptable way to start a review, list jobs, read findings or ask a question.
- Decision: BUILD `kaku review|jobs|findings|ask|chat --json` with NDJSON events, reading stdin, and exit codes that distinguish refusal, blocked disclosure and service error. These commands can request an approval but can never grant one: `kaku approve` stays interactive only. Exclude every yolo, auto and skip-permissions flag by design and say so in the help text (the Goose headless rule is the model: fail closed).
- Value: high (it is the integration and test surface for everything else).
- Test: `kaku review DIR --for X --json` prints events ending with `awaiting_approval`; with no TTY, `kaku approve` exits non-zero without contacting the approval route.

### A39 Configuration and diagnostics
- Reference: OpenCode layered config with schema (`config/index.tsx`, listed), `service get/set/unset`; Codex config layers and `/debug-config`; Hermes polls the config file's mtime every 5 s (README `useConfigSync`); Cline and Goose `doctor` / `configure`.
- Kakurega: LACKS. Only `--data-dir` and `NO_COLOR` (`main.rs`; `ui.rs`).
- Decision: BUILD a small TOML or JSON file in the data directory with a closed set of keys (notifications, verbosity, reduced motion, timestamps, keybindings from A33), a schema check that rejects unknown keys loudly, and `kaku doctor` that reports service state, version match and the log path. No key may relax a security setting.
- Value: medium.
- Test: an unknown key fails with the key path; no key in the schema can change approval, route or policy behaviour (assert against a list).

### A40 Status line, title, usage
- Reference: Codex configurable status line and terminal title items, `/status`, `/usage`; Hermes status bar and `useGitBranch`; Cline status bar tests; Goose reports a context-usage summary.
- Kakurega: PARTIAL. Status line shows route, readiness, review counts, running count (`app.rs` 242-264).
- Decision: IMPROVE modestly: add the current session name, a "keep/not kept" badge (A01), and "N approvals waiting" (A14). Set the terminal title to a fixed string; never put a recipient or a filename in the title, because titles are visible to window lists and screen shares.
- Value: medium.
- Test: title bytes contain no job data (assert on the pty stream).

### A41 Clipboard
- Reference: Hermes OSC 52 plus a child-process clipboard (`lib/osc52.ts`, `lib/clipboard.ts`, listed); Codex `clipboard_copy`, `clipboard_paste`, `markdown_copy`; OpenCode `useClipboard`.
- Kakurega: LACKS.
- Decision: BUILD copy as an explicit action only (see A31). Reading the clipboard on paste is already covered by bracketed paste; do not read the clipboard directly.
- Value: low to medium.
- Test: copy puts only the selected text; nothing is copied unless the user presses the key.

### A42 How the references test a terminal UI
- Reference: Cline runs the real CLI in a PTY backed by a terminal emulator (tuistory) and waits for text to appear instead of sleeping; an older suite pipes keystrokes through `script` on a fixed schedule and greps raw output, which the file header says is brittle (`cli.tuistory.e2e.test.ts` 1-60). OpenCode renders under a test renderer and asserts on UI regressions such as a thought group toggling after a backfill (`test/session-backfill-disclosure.test.tsx`). Codex keeps snapshot tests next to widgets (`snapshots/` directories, listed).
- Kakurega: PARTIAL. The reducer and renderer are tested well with `TestBackend` (TU01-TU18 by name, bodies not read); the pty layer is missing (see section 2).
- Decision: BUILD first. A PTY harness in Rust (a pty crate to run the real `kaku`, plus a terminal-state parser to read the screen, and wait-for-text with timeouts). Candidates exist; none was read here, so crate choice, licence and maintenance are NOT VERIFIED. Every item in the order below should add a harness test.
- Value: high.
- Test: the harness starts `kaku`, waits for the prompt, types `/help`, asserts help text on the emulated screen, sends Ctrl-C, and asserts the alt-screen was left and the cursor is visible; same under a forced panic.

### A43 Client/daemon split and shared service
- Reference: Hermes TUI is a client of a separate gateway over JSON-RPC (README 5-30), and the same gateway serves desktop and dashboard; the gateway declares each method's params and result as typed models and generates TypeScript, failing CI when stale (`AGENTS.md` "The wire is declared in Python and generated"). Codex runs an app-server daemon that several clients share (`daemon_startup.rs`, `daemon_recovery.rs`); `codex queue` refuses to run through an embedded server while a daemon exists, to avoid two writers. Cline has a hub daemon and a Zen mode that hands a task to it and exits.
- Kakurega: HAS the split and the one-service rule (`tui.rs`, `http.rs`). The TUI parses responses as untyped `serde_json::Value` with string keys (`app.rs` throughout), so a renamed field fails silently as an empty string or `?`.
- Decision: IMPROVE: a small shared `kk-api` types crate used by service handlers and the TUI (and the web client tests), with `deny_unknown_fields` on requests. Keep one writer: if a second `kaku` attaches to the same data directory, both pair as separate browser-like sessions today (`http.rs` 40-64); confirm that two terminals approving the same release cannot both succeed (INFERRED to be handled by the service; not tested here).
- Value: medium.
- Test: serialising a handler's response and parsing it in the TUI uses the same type; a field rename breaks compilation.

### A44 Safe rendering of untrusted text
- Reference: OpenClaw strips bidi controls and flattens whitespace in approval text (`tui-plugin-approvals.ts` 8-14) and sanitises renderable text; OpenCode strips ANSI and controls from notifications (`attention.ts` normalizeText); Codex filters control characters before adding trusted hyperlinks (`insert_history.rs` header).
- Kakurega: HAS and is at least as strict (`app.rs` 136-148, test TU08). One gap: `clean()` keeps `\n` and `\t`, and tabs are expanded to 4 spaces, which is fine; ANSI CSI sequences begin with ESC (a control character) so they are neutralised, VERIFIED by the `is_control` arm.
- Decision: keep; extend the same filter to notification text and terminal title (A11, A40), which are new output channels.
- Value: high.
- Test: a finding title containing ESC sequences and a bidi override appears escaped in cards, overlays, notifications and the title.

### A45 Context compaction and token limits
- Reference: OpenCode `compaction.ts` (1246 lines, NOT READ), `/compact` in Codex, Cline, Goose (summarise to prevent hitting the context limit, `slash_command.rs` description).
- Kakurega: PARTIAL. The client sends only the last 18 turns and the service caps 20 turns and 16000 characters (`app.rs` 723-727, `service.rs` 979-990).
- Decision: defer. A compaction summary is model output that becomes input, which makes it a taint path; design it in the kernel taint component first (`kernel-taint.md`, not read here). Until then show "older messages are not sent" in the transcript when the window drops turns, so the owner is not surprised.
- Value: medium.
- Test: the 19th turn shows a visible marker that turn 1 is no longer in the model's context.

### A46 Terminal-hosted extension surfaces (plugins, MCP manager, skills, voice, pets, updates, telemetry)
- Reference: Codex `/mcp`, `/plugins`, `/skills`, `/hooks`, `/voice`, `/pets`, update prompts; Cline `mcp install` wizard, plugins, skills picker; Hermes `/plugins`, `/skills`, `/billing`, `/voice`; OpenCode plugin API and `dialog-mcp.tsx`.
- Kakurega: LACKS.
- Decision: EXCLUDE from the terminal. Tool and skill management is the broker's job (one place that decides what may run); an install wizard in the terminal is a second path. Update prompts and telemetry are EXCLUDED: the product's promise is that nothing leaves without approval. Voice and pets: no value for the goal.
- Value: none.
- Test: not applicable; add a test that `kk-tui` has no network dependency except the loopback service address.

## 4. Prioritised implementation order (top 10)

Each step ends with harness tests from step 1 and a row in `TESTING.md`. Dependencies are noted.

1. PTY end-to-end harness and the missing tests (A42, A32, A36 terminal restore). Fixes the false claim in `terminal-workspace.md` line 21-22, and gives every later step a real-terminal check. Includes a canary check that nothing typed remains in scrollback after exit.
2. Service session store and the sessions UI (A01, A02, A03, A04), vault-backed, opt-in, with forget. Introduces the shared list-selection overlay widget. Adapt behaviour from OpenCode `dialog-session-list.tsx` and `dialog-fork.tsx` (MIT) and Hermes `methods_session_branch.py` (idempotency key). Reads `kk-vault` and `vault-storage.md` first.
3. Cancel and interrupt (A07, A08): service cancel route, `/stop`, Esc on pending chat, double-press quit guard. Goose `session/mod.rs` 1718-1760 is the pattern for leaving a valid conversation.
4. Streaming chat with a safe markdown buffer and renderer (A28, A29). Adapt the idea of Goose `session/streaming_buffer.rs` (Apache-2.0) after reading its body; Codex `markdown_render/` and `markdown_stream.rs` (Apache-2.0, ratatui) are the closer match to Kakurega's stack and must be read before choosing.
5. Composer rebuild: multi-line, grapheme-aware, paste chips and burst detection, external editor, vault-backed history with Ctrl+R (A19-A23). Candidate to adapt: Codex `bottom_pane/paste_burst.rs` and the search rules in `chat_composer_history.rs` (Apache-2.0).
6. Command table and popup, with job-id, memory-id and folder completion, and `@` references resolved by the service (A15-A18). Adapt OpenCode `prompt/frecency.tsx` (formula only) and the realignment idea of `mention.ts` (MIT).
7. Queue and pending-input strip (A06), then `/aside` (A25). Rule: only plain chat queues. Take the display rules from Codex `pending_input_preview.rs`.
8. Notifications, terminal title, job dock, resume-time catch-up (A09-A11, A40, A44). Adapt Codex `notifications/{mod,osc,bel}.rs` after reading the bodies (only the header was read), and the OpenCode event rules in `notifications.ts`.
9. Performance and long-output: wrapped-line cache, draw-on-change, collapsible cards, verbosity levels, `?since=` job events, transcript overlay with search and explicit copy (A30, A31, A37). Port Hermes `event_replay.py` semantics and Cline's tuistory-style assertions.
10. Approval hardening and diff widget: dismiss on external resolution, refuse-with-reason, an approvals-waiting count using the OpenClaw snapshot-journal pattern (MIT, `tui-snapshot-journal.ts`), diff rendering inside the overlay and in `/fixes`, and the headless `--json` commands that can request but never grant approval (A12, A14, A27, A38). Port Hermes `gatewayRecovery.ts` (MIT) for the child-service restart budget (A36).

Deferred behind these: rewind and `/undofix` (A05), keymap and palette (A33), config file and `kaku doctor` (A39), linear accessibility mode and VoiceOver pass (A35), shared `kk-api` types (A43), compaction (A45), themes (A34).

Permissively licensed code worth adapting, by file (all require reading the body first, which this study did not do beyond what the Sources table lists):
- Codex (Apache-2.0, ratatui, closest stack): `tui/src/bottom_pane/paste_burst.rs`, `bottom_pane/chat_composer_history.rs` and `chat_composer_history/search_batch.rs`, `bottom_pane/pending_input_preview.rs`, `notifications/mod.rs`, `osc.rs`, `bel.rs`, `markdown_stream.rs`, `markdown_render/`, `diff_render.rs`, `external_editor.rs`. Most import `codex_*` crates, so expect a port, not a copy.
- Goose (Apache-2.0, Rust): `crates/goose-cli/src/session/streaming_buffer.rs`, `session/editor.rs`, `session/paste.rs`.
- OpenCode (MIT, TypeScript, so algorithm ports): `prompt/history.tsx` (dedupe, cap, self-heal), `prompt/frecency.tsx`, `prompt/mention.ts`, `component/prompt/draft-stash.ts`, `core/src/session/inbox.ts` (steer versus queue promotion), `core/src/permission.ts` (`evaluate`, last match wins, for the policy owner), `mini/verbosity.ts`.
- Hermes (MIT, TypeScript and Python): `ui-tui/src/app/gatewayRecovery.ts`, `tui_gateway/event_replay.py`, `lib/history.ts`.
- OpenClaw (MIT, TypeScript): `src/tui/tui-snapshot-journal.ts`.
- Cline (Apache-2.0): the tuistory test approach in `apps/cli/src/cli.tuistory.e2e.test.ts`; the checkpoint counting lesson in `tui/checkpoint-picker-items.ts`.
Every adoption gets a NOTICE.md entry and no comment in code.

## 5. Unknowns and limits

- Reading depth is stated per file in section 1. Anything marked "listed" in the matrix was seen in a directory listing only and was not read.
- The Codex clone named in the brief (f2b94ef) does not contain `tui` or `app-server`. Findings about Codex come from the sparse clone at 021fccb, which is a different commit. App-server behaviour (thread list pagination, queue API) is OBSERVED from client code and doc comments only.
- I did not run any of the reference products. All behaviour claims are from reading code, doc comments and READMEs; none are measured. Hermes' README is a specification written by its authors; where I rely on it I did not confirm the code does what it says except for `gatewayRecovery.ts` and the parts of `useSubmission.ts` named.
- The OpenCode double-press interrupt is inferred from a counter and a 5000 ms reset; I did not run it.
- I did not read the vault, the policy engine, the kernel taint component, or the TUI test bodies, so whether sessions fit `kk-vault`, whether policy already has an unattended rule, and what TU01-TU18 actually assert are UNKNOWN.
- The rest of `app.rs` (1160-1532) was only grep-located, so details of the Fixes, ApplyFix, Ask and Chat response rendering are UNKNOWN.
- Terminal-feature claims (OSC 9 and 777 support, bracketed paste on Windows, VoiceOver behaviour) are NOT VERIFIED on this Mac; the harness in step 1 should establish what the target terminals actually do.
- Crates named as candidates (a pty crate, a terminal-state parser, `pulldown-cmark`, `syntect`, `unicode-segmentation`) were not evaluated for licence, size or maintenance. None is a decision.
- Candidate clients not studied: Gemini CLI, Aider, Crush, Amp, Claude Code (closed). They may hold ideas on diff review and session handling that are missing from this matrix; this study does not claim coverage of them.
- Surfaces not covered: the web and desktop equivalents of these capabilities, the Telegram bridge, the browser extension, and the MCP surface. This matrix says nothing about whether those match the terminal.
- Two judgement calls for the owner are flagged, not decided: whether pasting into the approval digest field should be allowed (A12), and whether the default should stay alt-screen (recommended, A32).
