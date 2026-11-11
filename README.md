# Kakurega

A confidential AI workspace. An AI works on your private files and code, a security layer outside the model decides what may leave, and the approved result reaches a recipient as a capsule they can check themselves.

Status: working prototype on macOS and Linux. Not a finished product: see [docs/LIMITS.md](docs/LIMITS.md) before relying on it.

## What it does today

- Review a project privately: an agent reads it inside a sealed sandbox, secrets are withheld from it, and the kernel re-runs each finding's reproducer on a minimal extract. Only confirmed findings can be released.
- Keep chosen files out of every review: list them in a `.kakuignore` file at the top of the project (names, folders and `*`/`?`/`**` patterns, like a `.gitignore`); they are withheld from the model and from the release like secrets, and a list that cannot be read withholds everything.
- Release only what you approve: you see the exact files, the cumulative disclosure to that recipient and what the release does not establish, then approve in the desktop window (Touch ID prompt), the terminal workspace or `kaku approve`.
- A signed capsule the recipient verifies offline, in a browser at `/verify` or with `kaku verify`, with a second independent verifier in JavaScript.
- Private memory and chat on your computer: notes with privacy labels, a reason and a history; forgetting deletes the text for good; chat uses notes as data and keeps instruction-like notes away from the model.
- A signed, hash-chained activity log you can verify anywhere (`kaku log verify`).
- Web research you approve first: you see the exact addresses (or the exact search words and the one site that would receive them) before anything is requested; pages and search results reach the model only as untrusted data. Search needs a provider you choose (`KAKUREGA_SEARCH_URL`, an https SearXNG-style endpoint that answers `format=json`, which a stock SearXNG does only after `json` is added under `search.formats`); none is bundled. Public SearXNG sites mostly rate-limit or turn JSON off, so a site you run yourself is the dependable choice.
- An encrypted backup of all your data with a recovery key (`kaku backup`, `kaku restore`), notes brought in from other assistants (web Memory screen or `kaku memory import`), notes that belong to one project, and recall by meaning when an embedding model is installed (`ollama pull nomic-embed-text`).
- Signed update announcements that can be checked offline (`kaku update check|verify-file`). Nothing downloads or installs updates yet.
- Surfaces: browser workspace (English and Hindi), full-screen terminal workspace (`kaku`), desktop app (Electron), MCP server (`kaku mcp`: counts and statuses; the tools that start a review or ask for approval stay behind owner leases with `--leased`, see `docs/mcp.md`; every call is recorded in the signed log), a minimum-permission browser extension, a read-only Telegram bridge.
- A page that says where your data goes (what stays on this computer, what can leave, and when), read from the running service; notes that say almost the same thing can be merged by you, with undo; what these public documents promise is checked against the code by `python3 scripts/docs_check.py`.
- Working with documents and meetings: ask a folder of documents and get an answer with quotes that are checked against the files; turn a meeting transcript into decisions, actions and open questions where each point is kept only if the words it quotes are in the transcript (`kaku notes FILE`); share one finding of a delivered review with a proof that shows nothing else of the release (`kaku share`).
- What is held back from a model: lines that hold a key, a token or a password said in a sentence, and lines that read like orders, in attached files, folders and transcripts, whichever model runs. When the model is not on this computer, the names you list and any email address, phone, card, account or identity number are sent as placeholders and put back in the answer you read. What is recognised and what is not is measured, not asserted: see the leak benchmark below.
- A leak benchmark you can check and repeat: `benchmarks/leak/METHOD.md` describes 168 cases against a model that repeats any planted secret it is shown; the results are signed (`kaku bench verify`), and `benchmarks/leak/reverify --rerun` runs it again on your computer. Its known leaks (a password or code said with no word for what it is) are listed in the results. In the app, "Try to break it" lets you plant your own text next to fresh secrets and see exactly what the model was sent.
- A review that looks twice: when the model says it found nothing, each source file is taken on its own (what could be wrong here, then a script to prove it), and only what the sandbox confirms is kept. `benchmarks/quality/METHOD.md` has the small eval behind this and its limits.
- More in the workspace: projects with their own folder, notes and instructions; saved prompts; a roundtable of experts and team runs; a list of what left this computer, read from the signed log; sample files to try everything without your own; the route (this computer or NEAR AI Cloud with your own key) chosen in Settings.
- Outside help only with your yes: a read-only GitHub connector that signs in with a code and turns an issue or file into an attachment that passes the same checks as any file (it needs the client ID of a GitHub App of your own; tested against a stand-in, not yet against github.com); and "Ask Claude Code", which shows you the exact text first and then runs the Claude Code installed on your computer on that text alone, with its tools off. That hand-over is not private and says so.
- Profiles: `kaku profile new work` makes a second, separate set of keys, memory, skills, connectors and settings on the same computer; `--profile work` or `kaku profile use work` chooses it, the app shows which one is in use, and the desktop menu switches between them. Nothing is shared between two profiles.
- In the desktop app: take a picture of the screen for the chat from the menu-bar menu; you see the picture and say yes before it is attached, and the page itself can never capture the screen.
- Suggest a skill from a conversation: a way of working you taught in a chat can be written up as a skill; it waits under Skills and nothing uses it until you keep and approve it.
- Models: a local model through Ollama, or a model on NEAR AI Cloud's confidential hardware (`kaku serve --route near --near-model NAME`, the account key in `NEAR_AI_API_KEY` or `--near-key-file`). The hardware proof of NEAR's gateway and model is verified for real, with no key (VERIFIED by run); a real completion, its end-to-end encryption and its signed response are built and tested against a simulated gateway and are NOT yet run live, because that needs an account key. Coding tasks, embeddings and model choice stay with the local model.

## Run it (macOS)

```
scripts/setup                        # dependencies, web build
ollama serve &                       # and: ollama pull qwen2.5:3b
limactl start kak                    # Linux VM with bubblewrap and Rust (the sandbox on macOS)
cargo build -p kk-cli
target/debug/kaku                    # full-screen workspace; starts the service if needed
target/debug/kaku serve              # or the browser workspace: prints a one-time pairing link
```

Desktop: `cd apps/desktop && pnpm install && pnpm start` with `KAKU_BIN` pointing at the built `kaku`. A self-contained macOS app is built by `scripts/package-desktop-macos` (a zip and its SHA-256) and installed by `scripts/install-desktop-macos`, which checks the SHA-256 before it unpacks anything; the app is signed ad hoc only and is not notarized. It stays in the menu bar and tells you when something waits for you.

Linux package: `scripts/package-linux` builds and checks a `.deb` in the Linux VM and copies it to `target/packaged-linux/` (install with `sudo dpkg -i`, start with `kakurega`).

## Check it

```
scripts/test                         # lints, unit and integration tests, layer rule, secret scan (not the end-to-end scripts)
scripts/e2e                          # a real browser drives a full review, approval, memory, chat and log
scripts/e2e-all                      # every end-to-end script: browser, terminal, MCP, Telegram, extension, desktop, signed approvals
scripts/test-lima                    # the sandbox and worker tests inside the Linux VM
python3 scripts/verify_program.py I  # the acceptance gate for a milestone (also II and III)
```

`program/ACCEPTANCE_MANIFEST.json` lists every acceptance row with the command that proves it; rows that need a person or an external key say so. `TESTING.md`, `PROGRESS.md` and `HUMAN.md` (one level up in the working tree) record evidence, state and what only the owner can do.

## Check a capsule as a recipient

```
scripts/judge-replay                 # builds the check page, makes a good and a forged capsule, checks both from the command line
```

The page `apps/verifier-page/dist/kakurega-verify.html` is one file with no network use: open it from disk in any browser, drop a capsule on it, and optionally paste the signer key you expect. It lists each model signature in the capsule and says what the check does not establish, including that a model's key is not shown to belong to attested hardware.

## Layout

- `crates/` Rust crates (trusted core). The layer rule in `scripts/layers.json` is checked by `scripts/check_layers.py`.
- `apps/` web workspace, desktop app, browser extension, the standalone capsule check page.
- `contracts/` the NEAR capsule registry contract (testnet only; a separate Cargo project built with `cargo near`).
- `packages/` shared TypeScript packages (canonical JSON, capsule verifier).
- `fixtures/`, `tests/` test inputs and cross-component tests.
- `docs/engineering-decisions/` one record per component: target experience, what was read, decision, evidence, what is not covered.
- `docs/mcp.md` the MCP server. `docs/LIMITS.md` what is and is not claimed.
- `program/` acceptance manifest and the parity registry of every planned capability.

## Requirements

git, Rust (version pinned in `rust-toolchain.toml`), Node 22.19 or newer, pnpm, Python 3. A recipient who gets a release with a JavaScript finding needs Python 3 and Node.js 22 or newer to re-run its check; without Node the runner says so and still checks the other findings. macOS also needs Lima for the sandbox; Linux needs bubblewrap. Windows is not supported.

## Licence

The licence of Kakurega itself has not been decided, so the repository is private. Third-party credits are in `NOTICE.md`.
