# Standing instructions: the owner's own orders to the assistant (ledger G-GH-19)

## Target experience
- The owner writes, once, how the assistant should answer (how short, what language, what role) for every conversation and, optionally, for one project. They are used on every chat question without being repeated, and the owner can see that they were.
- They are orders, not facts, and they stay separate from saved notes: nothing a document, web page, review, note, agent tool or browser extension says can add to them or change them.

## Sources read
- OpenCode `packages/core/src/config/plugin/instruction.ts` (MIT, commit fe04656): READ IN FULL. It loads `AGENTS.md` from a global config folder and from every folder between the working directory and the project root, plus configured paths and URLs, watches them and treats all of it as authoritative instructions. Weaknesses for us: a project's own `AGENTS.md`, or a remote URL, can carry instructions into the model with the owner's authority, so a hostile repository gets to give orders; there is no size limit, no secret check and no record of what was sent.
- Ghast: the public changelog names the feature ("personal AGENTS instructions for chat and coding tasks") and nothing more; no source is available for it (the clone has docs only). NOT READ: Claude Code `CLAUDE.md` loading, Codex `AGENTS.md` discovery, Cursor user rules.

## Decision: BUILD, and beat the file-based design
- The instructions live only in the encrypted store and are written only through the owner's browser session or terminal (`POST /api/instructions`). There is no admin route, no agent tool and no extension route for them (test sv86). A project's own `AGENTS.md`, a document, a web page or a note stays untrusted data and never becomes an instruction.
- Text is refused if it is longer than 4,000 characters, holds control, direction or Unicode tag characters, or looks like a secret (they are sent to the model on every question). Up to 30 projects can have their own. An empty text removes the instruction. The signed log records that they changed and how long they were, never what they said.
- Each chat question gets one extra system message right after the fixed rules, saying what the block is and what it cannot do (it cannot change what the assistant can see, send or do, never overrides the fixed rules, and nothing in notes, pages or documents can add to it), then the global text, then the named project's text. The answer reports `instructions_used`, the web footer says "your instructions applied", and the context meter counts the block.
- Not applied to document questions (`ask`) or reviews: those have strict output formats the checks depend on, and an owner order such as "reply in French" could break them. Stated, not measured.

## Evidence
- kk-service sv84 (CRUD, 11 refused forms, 30-project limit exact, log), sv85 (what the model receives: global only, global then project, no other project, removed, a saved note called "standing instructions" stays a note), sv86 (no other door); kk-tui tu41 (`/instructions`, `set`, `clear`, secret scrubbed from history); browser e2e 243/243 (save, reload keeps it, secret refused, chat footer says so, removing it). 18 mutants, all caught after two tests were made exact (the 30 limit, the dot names).
- Not covered: the order of this block relative to a fetched web page block (the code puts the owner's block first; no test fetches a page and checks); a real model's behaviour under the instructions; Hindi strings read by a person; Windows.
