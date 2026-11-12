# Terminal workspace: the `kaku` full-screen interface (registry N69, H9 partly; Milestone III)

## Target experience
Typing `kaku` in a terminal opens a full-screen, keyboard-first workspace: a status line that says what is private and ready, a transcript of what has happened, one composer with slash commands and tab completion, live progress of a running review, and an approval view that shows the exact files, the cumulative disclosure and what is not established, and releases nothing until the owner types the digest prefix. It resumes whatever the service already holds (reviews, memory, log). Scripts keep using plain subcommands.

## References: what was actually read (and what was not)
- Codex TUI (Apache-2.0, sparse clone of `codex-rs/tui` at commit 021fccb in `private-ai/inspiration/codex-tui-src`): about 481,000 lines including tests. READ in full: `styles.md`; the structure and the approval logic of `bottom_pane/approval_overlay.rs` (request queue, modal key capture, per-option shortcuts, explicit Cancel as a decision that also clears the queue, externally resolved requests dismissing the overlay, footer key hints). NOT READ: the composer (12.9k lines), textarea, run loop (`lib.rs`), keymap, markdown renderer, streaming controller, history cells, resume picker. This is a small part of the whole; the rest is not claimed.
- Hermes `ui-tui` (MIT, about 119k lines): README read only. Idea adopted: the screen is a client; the process that owns sessions, tools and models is separate and spoken to over a narrow protocol. Source NOT read.
- OpenCode `packages/tui` (MIT, about 115k lines): only sized, NOT read. Its OpenTUI/Solid stack needs a JavaScript runtime.
- Decision: do not fork any of them. Codex's TUI is coupled to its own app-server protocol and is two orders of magnitude larger than what the kernel needs; Hermes and OpenCode would add a second language runtime to a Rust product. Build a Rust ratatui client inside the single `kaku` binary and adopt the patterns above. Ratatui and crossterm are MIT.

## Chosen approach
Crate `kk-tui`: a pure state machine (`App`) that takes key and response events and returns requests; a renderer over ratatui; an HTTP backend that talks to the same `/api/*` endpoints as the web app (the terminal pairs like a browser and uses `/admin/approvals` for approval, the same trusted path as `kaku approve`). One backend, no second set of business logic. Requests run on a worker thread so the screen never freezes.

## Where we aim to beat the references (to be proven, not claimed)
- Approval needs deliberate friction: typing the digest prefix, not one key; Esc or Ctrl-C is an explicit refusal that is recorded.
- The approval view shows cumulative disclosure and the not-established list, not only the command or patch.
- Meaning never depends on colour alone; `NO_COLOR` is honoured; every state has a word or symbol.
- The terminal is restored on every exit path, including a panic.

## Quality criteria (tests TU01 onward in `crates/kk-tui/tests` and `crates/kk-cli/tests/tui.rs`)
Command parsing including hostile input; the reducer for every command with fake responses; rendering with a test backend at wide and narrow widths (nothing overflows, key hints present, no colour-only state); the approval flow refuses a wrong digest and Esc refuses; input control characters cannot reach the screen as escape sequences; a real run against a live service drives start, progress, findings, release, approval and delivery; a real pseudo-terminal run starts, draws, and leaves the terminal clean on exit.

## Not covered by this component yet
Free-form chat with a model (H9 is not built: the composer accepts commands, not conversation), markdown rendering of model text, mouse support, themes, a command palette, Windows terminals, screen-reader testing.
- Test as used, with the real local model and the real sandbox, in a real pseudo-terminal (`tests/e2e/used_terminal.py`, run by hand; qwen2.5-coder:7b): open, `/model`, a question answered in 1.3 s, `/where`, a review of the sample project finished ready after 146 s with no key pressed, `/findings`, `/approve` with the digest's first characters, `/deliver`, `/where` again counting one release and nothing else, quit with the terminal given back. 12 of 12.
- Found by that walk and fixed:
  - `/where` did not exist, although the owner's description of this workspace names it. It is built (`/route` is the same): the model and where it runs, each door (web search, Telegram, the extension, GitHub, the coding agent), the notes, and, from the signed log, what left this computer and what was held back. Evidence: `tui_pty.py` 118 (2 new), the real walk.
  - `/findings` listed the kernel's confirmed findings first, so they scrolled away and the screen ended on suspicions marked "NOT reproduced". A count line now comes first and the confirmed findings are listed last, where the eye is.
- The walk's own first version had two false passes (it matched "ready" in the status line and "this computer" in the greeting); its waits now name the exact lines.
