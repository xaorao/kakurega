# What is and is not claimed

This file is the plain list. `docs/TESTING.md` says which test checks what and what runs only by hand; `program/ACCEPTANCE_MANIFEST.json` gives each acceptance row the command that proves it; the status of every planned capability is in `program/parity_registry.json` (most are not built).

## Not verified, and why
- NEAR AI Cloud completions, encrypted inference, signed responses and image provenance: the NEAR API key and credits do not exist yet. Only model attestation was verified, against real public evidence.
- Quality of any real model's review or chat. Local small models find few defects; no bake-off was run.
- A person other than the builder using the product unaided. No first-time-user test has been observed.
- Real Touch ID. The desktop approval flow is tested with the operating-system prompt stubbed.
- Telegram: the bridge was checked once on a real phone (pairing, a status reply and a review notification, observed by the owner) and against a fake Telegram server. Delivery retries, flood control and 4xx/5xx handling are unit-tested only; long-running reliability (days, reconnects, many chats) is not tested. Chat is a transport for counts and ids: approvals are never done through chat.

## Known weaknesses
- An agent that runs commands as the owner (an MCP client with a shell) can fake a terminal and read the service's files, so `kaku approve` refusing a piped digest only slows it down. Approvals are bound to the owner only with an approval key that asks for them (`kaku approver enroll --enclave`, Touch ID); without one, an agent with a shell can approve. See `docs/mcp.md`.
- A file attached through the browser is matched to a project by its exact bytes (up to 100,000 entries looked at across the project folders, `.git`, `node_modules`, `target` and virtual environments left out). An edited or re-saved copy of a project's file is not recognised as the project's.
- macOS runs the sandbox inside a Linux virtual machine; that is a development arrangement, not a hardened macOS sandbox.
- The prompt-injection defence for notes is a phrase list plus labelling. Rewording can get past the phrase list; the chat has no tools, which limits the damage.
- Cumulative disclosure checks file coverage and release rate only. It does not detect excerpts that combine into something no single file shows, and says so on screen.
- The event log lives on the same machine as its key. Entries removed from the end are only detectable with an anchor kept elsewhere.
- Hindi text was written by the builder and not reviewed by a native reader. Text that comes from the server stays English.
- A secret detector is heuristic. A wallet recovery phrase is recognised as twelve or more words in a row from the standard English list (BIP-39), or as a run of exactly 12, 15, 18, 21 or 24 plain words after "seed phrase", "recovery phrase" or "mnemonic"; a phrase in another language, split over lines, or with other words mixed in is not.
- A password made of ordinary words ("the password is purple elephant", or one plain word shorter than sixteen letters) is not recognised: it reads like any sentence. After "password is" or "password:" a value is recognised when it mixes letters with digits or signs, is digits only (four or more), or is one run of sixteen or more letters. After "secret" or "token" a number is not taken for a secret ("tokens: 4096" is ordinary), and a run of letters only when it is twenty-four or longer.
- A password or a code said with no word for what it is ("type Blue42 when it asks") is not recognised and is sent like any other line. The leak benchmark counts these: 10 of its 42 cases outside the known shapes reach the model (`benchmarks/leak/METHOD.md`).
- Names are hidden from a model elsewhere only when you listed them; a name that is not on the list is sent as written. Hiding applies to chat, roundtable, team runs, document questions and meeting notes, not to reviews, and not to text you hand to Claude Code: that text goes as you read and approved it.
- The NEAR account key typed in Settings is kept in a file only you can read, not in the system keychain.
- A conversation or a team run that worked with the model on this computer is not continued with a model elsewhere (it may hold notes and files that never leave); you switch back or start a new one. The level is recorded per conversation and per run, not per message, so this is stricter than it strictly has to be.
- "Ask Claude Code" is not private: the text you approve goes to Anthropic through your own Claude Code. It is run with none of your Claude Code settings (no hooks, no personal instruction files). In a team run the text offered for it holds the task and the earlier experts' results, which can quote a file you attached; a review's findings are never in it. If the service stops while the agent works, the answer is lost and the text is not sent again.
- The second look of a review takes each file on its own, so a defect that needs two files to see is not found by it; it looks at 12 files at most.
- The Electron desktop app is large (about 250 MB, about 450 MB memory); Tauri was not measured.

## Deliberately not built
- Approving a release through a chat service: the chat service would receive the exact bytes. Phone approval needs a native passkey app.
- An MCP tool that returns source, findings, memory or chat text: an MCP client is treated as an untrusted sink.
- Hosted mode, Windows, an SDK package, collaboration, a plugin store, voice, computer use. (A Linux `.deb` and a registry contract on NEAR testnet are built; see the README.)

## Process limits
- The builder reviewed their own work. Two audits by a separate reviewer that was given the code and no conclusions were also done; every finding, with its verdict and fix, is in `docs/engineering-decisions/audit-validation.md`. No outside person has reviewed it. Mutation checks were used where possible and found real test gaps several times.
- Classifications and trust reasons in the parity registry are the builder's judgement.
