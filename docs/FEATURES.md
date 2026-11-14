# What Kakurega does today

The full list, one capability per line. The short version is in the [README](../README.md); what is not claimed is in [LIMITS.md](LIMITS.md).

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
- Keep a project on this computer: each project says where its work may go (the model you choose, only confidential hardware, or only a model on this computer). With a less private model in use, nothing of that project is sent: its chats, kept conversations, document questions, reviews and team runs are refused in words until a more private model is chosen.
- Ask about a project's code: an agent reads the project (secrets withheld, nothing changed), with the model on this computer or in confidential hardware whose proof is checked, never a hosted model without that proof, and never a project kept on this computer and answers in plain words; every line its answer rests on is checked against the file and marked found or not found, and a short script it writes can be run in the sealed sandbox, with the script and what it printed shown next to the answer. In the browser (Ask a project), the terminal (`/explore`), `kaku explore`, and as the Code reader in a team run. A line found shows the answer rests on real code, not that the answer is right.
- Working with documents and meetings: ask a folder of documents and get an answer with quotes that are checked against the files; turn a meeting transcript into decisions, actions and open questions where each point is kept only if the words it quotes are in the transcript (`kaku notes FILE`); share one finding of a delivered review with a proof that shows nothing else of the release (`kaku share`).
- What is held back from a model: lines that hold a key, a token or a password said in a sentence, and lines that read like orders, in attached files, folders and transcripts, whichever model runs. When the model is not on this computer, the names you list and any email address, phone, card, account or identity number are sent as placeholders and put back in the answer you read. What is recognised and what is not is measured, not asserted: see the leak benchmark below. The list of names and each project's level ("only this computer", "confidential hardware too", "any model") are set in the web workspace (Settings, Projects) or in the terminal (`/hide`, `/private`).
- A leak benchmark you can check and repeat: `benchmarks/leak/METHOD.md` describes 168 cases against a model that repeats any planted secret it is shown; the results are signed (`kaku bench verify`), and `benchmarks/leak/reverify --rerun` runs it again on your computer. Its known leaks (a password or code said with no word for what it is) are listed in the results. In the app, "Try to break it" lets you plant your own text next to fresh secrets and see exactly what the model was sent.
- A signed receipt under every chat answer from confidential hardware (the other tools check each answer's signature the same way before using it and say where the model ran, but do not show the receipt): the model's own signature over the exact bytes sent to it and sent back (encrypted, as two fingerprints), checked before the answer is shown, kept with the conversation, and copyable for someone else to check against the key the hardware proof vouches for.
- Choosing the NEAR model from NEAR's own list in Settings: each chat model with whether its hardware proof can be checked (one without cannot be chosen), whether it can run reviews, its prices and its window.
- Review a change, as for a pull request: `kaku review DIR --for WHO --change main` (also on the web's New review and as `/review FOLDER for WHO against main`) reviews what the checked-out commit changes relative to a branch, tag or commit. The change is read from the local repository without running git, the committed version is what is reviewed, the agent is told which lines changed, and only confirmed findings that cite a changed file or whose check runs one are offered. The release states the two commits and a digest of the change, and a delivered review has "Copy as a pull-request comment". Nothing is sent to GitHub.
- A review that looks twice: when the model says it found nothing, each source file is taken on its own (what could be wrong here, then a script to prove it), and only what the sandbox confirms is kept. `benchmarks/quality/METHOD.md` has the small eval behind this and its limits.
- More in the workspace: projects with their own folder, notes and instructions; saved prompts; a roundtable of experts and team runs; a list of what left this computer, read from the signed log; sample files to try everything without your own; the route (this computer or NEAR AI Cloud with your own key) chosen in Settings.
- Outside help only with your yes: a read-only GitHub connector that signs in with a code and turns an issue or file into an attachment that passes the same checks as any file (it needs the client ID of a GitHub App of your own; tested against a stand-in, not yet against github.com); and "Ask Claude Code", which shows you the exact text first and then runs the Claude Code installed on your computer on that text alone, with its tools off. That hand-over is not private and says so.
- Profiles: `kaku profile new work` makes a second, separate set of keys, memory, skills, connectors and settings on the same computer; `--profile work` or `kaku profile use work` chooses it, the app shows which one is in use, and the desktop menu switches between them. Nothing is shared between two profiles.
- In the desktop app: take a picture of the screen for the chat from the menu-bar menu; you see the picture and say yes before it is attached, and the page itself can never capture the screen.
- Suggest a skill from a conversation: a way of working you taught in a chat can be written up as a skill; it waits under Skills and nothing uses it until you keep and approve it.
- Models: a local model through Ollama, or a model on NEAR AI Cloud's confidential hardware (`kaku serve --route near --near-model NAME`, the account key in `NEAR_AI_API_KEY`, `--near-key-file` or typed in Settings). The hardware proof of NEAR's gateway and model is checked before each request, every question is encrypted to the model, and every answer's signature is checked against the attested key before it is used (VERIFIED live with `Qwen/Qwen3.8-27B`). Reviews, project questions, coding tasks, teams, chat, documents and meeting notes all run on it; document search by meaning stays with the local model. Activity shows what each route cost, at NEAR's published price unless you entered one.
- Releases can be made valid for longer than a day: `kaku serve --release-days N` (1 to 730). The public site's sample capsule (`fixtures/near-live/sample.kkg`) is a real review on NEAR AI Cloud made valid for a year.

## The state of each feature

**Works** is the builder's record that the feature works, with how it was checked where the row says. **Built** means it is in the product, with its test named where one exists; this page claims nothing more. The full list is in [docs/FEATURES.md](FEATURES.md).

**Review and release**

| Feature | State |
| --- | --- |
| Private review in a sandbox, findings re-run by the kernel, release by digest, signed capsule | Works; walked in a browser with a real local model |
| Review a change, as for a pull request: `kaku review DIR --for WHO --change main`, **New review** on the web, or `/review FOLDER for WHO against main` in the terminal. The release states the two commits and a digest of the change; a delivered review has **Copy as a pull-request comment**. Nothing is sent to GitHub | Built (`tests/e2e/change_web.mjs`, `tests/e2e/change_review.py`). A change of more than 300 files or 2 MB is refused, not cut. Uncommitted edits are not reviewed |
| A second look: when the model finds nothing, each source file is taken on its own, and only what the sandbox confirms is kept | Built; one file at a time and 12 files at most, so a defect that needs two files to see is not found by it |
| `.kakuignore`: files and folders you name are withheld from the model and from the release, like secrets | Built; a list that cannot be read withholds everything |
| Releases valid for longer than a day: `kaku serve --release-days N` (1 to 730) | Built; the public site's sample capsule is a real review on NEAR AI Cloud made valid for a year |
| Recipient check: one-file page, `kaku verify`, a second verifier in JavaScript, optional lookup on NEAR | Works |
| Share one finding of a delivered review with a proof that shows nothing else of the release (`kaku share`) | Built (`tests/e2e/share_finding.py`) |

**Models, documents and memory**

| Feature | State |
| --- | --- |
| Chat, documents with checked quotes, meeting notes with checked quotes (`kaku notes FILE`), memory with labels | Works with a local model |
| Ask about a project's code: every line the answer rests on is checked against the file and marked found or not found, and a short script the agent writes can run in the sealed sandbox. In the browser (Ask a project), the terminal (`/explore`) and `kaku explore` | Built (`tests/e2e/explore_web.mjs`). A line found shows the answer rests on real code, not that the answer is right |
| Secrets and spoken passwords withheld; names swapped for placeholders when the model is not on this computer | Works; measured by the leak benchmark |
| Each project says where its work may go: only this computer, confidential hardware too, or any model. With a less private model in use, nothing of that project is sent | Built |
| Model on NEAR AI Cloud | Works live with `Qwen/Qwen3.8-27B`; other models there untested |
| The model's signature under every answer from confidential hardware: over the exact bytes sent and received as they travelled (encrypted), checked before the answer is shown, kept with the conversation, copyable for someone else to check | Works live with `Qwen/Qwen3.8-27B` |
| Choosing the NEAR model from NEAR's own list in Settings: whether its hardware proof can be checked (a model without one cannot be chosen), whether it can run reviews, its prices and its window | Built (`tests/e2e/near_models_web.mjs`); only `Qwen/Qwen3.8-27B` was run live |
| Web research you approve first: you see the exact addresses, or the exact search words and the one site that would receive them | Built; search needs a provider you choose (an https SearXNG-style endpoint); none is bundled |
| Roundtable of experts, team runs with a lead, a Project reviewer seat that runs the sandboxed review | Works with a local model |
| Skills: your own, a bundled shelf, signed packs and a publisher's signed catalog; nothing is used until you approve it | Works |
| Profiles with separate keys; encrypted backup with a recovery key (`kaku backup`, `kaku restore`) | Works |
| Hand a question to Claude Code, with your yes on the exact text | Works; said plainly to be not private |
| GitHub connector (read only) | Tested against a stand-in; not yet with a real GitHub App |

**Surfaces and platforms**

| Feature | State |
| --- | --- |
| Full-screen terminal workspace (`kaku`), MCP server (`kaku mcp`), browser extension, Telegram bridge | Works |
| Desktop app (macOS, Linux): native approval with Touch ID, menu-bar item, notifications, a picture of the screen for the chat; on macOS, your calendar and open reminders attached on request | Works; unsigned build, the capture preview was not clicked by a person in a test, and reading a real calendar waits for one Allow from the owner |
| A page that says where your data goes (what stays on this computer, what can leave, and when), read from the running service | Built |
| Signed update announcements checked offline (`kaku update check`, `kaku update verify-file`) | Built. `kaku update download` fetches an announced file and checks it against the signed announcement; nothing is opened, run or installed, and nothing checks for updates by itself |
| Hindi | Written by the builder, not reviewed by a native reader |
| Windows, mobile, hosted mode, encrypted sync between devices | Not built |

<details>
<summary><b>Claims, code and tests</b></summary>

| Claim | Code | Tests |
| --- | --- | --- |
| A key or password is withheld before any model sees it, in every encoding the detector reads | `crates/kk-secrets/src/lib.rs`, `crates/kk-secrets/src/spoken.rs` | `crates/kk-secrets/tests/secret_grid.rs`, `scripts/bench-leak.py` |
| Nothing is released without your approval of that exact digest | `crates/kk-service/src/service.rs`, `crates/kk-evidence/src/approval.rs` | `crates/kk-service/tests/flow.rs`, `tests/e2e/run.mjs` |
| A finding is kept only when its script shows the normal behaviour first, prints the same twice, and no longer reproduces with the cited file emptied | `crates/kk-review/src/check.rs`, `crates/kk-review/src/job.rs` | `crates/kk-review/tests/check.rs`, `crates/kk-review/tests/job.rs`, `scripts/eval-quality.py` |
| A review of a change covers the change it names: one over 300 files or 2 MB is refused, never cut; a single file over 512 KB, a link or an unsafe name is left out and named in the release | `crates/kk-git/src/lib.rs` | `crates/kk-git/tests/change.rs`, `tests/e2e/change_review.py` |
| A capsule that was changed, or signed by someone else, is refused by both verifiers | `crates/kk-evidence/src/capsule.rs`, `packages/verifier-js` | `tests/e2e/verifier_page.mjs`, `scripts/judge-replay` |
| The review agent has no network and sees no file you excluded | `crates/kk-sandbox`, `crates/kk-broker` | `scripts/test-lima`, `scripts/test-linux` |
| The desktop page cannot be given the camera, the screen, or any other permission, and cannot navigate away | `apps/desktop/src/policy.mjs`, `apps/desktop/src/main.mjs` | `tests/e2e/desktop_attack.mjs` |
| Text reaches the coding agent on your computer only after your yes on that exact text, once, even across a restart | `crates/kk-service/src/service/team.rs`, `crates/kk-cli/src/handoff.rs` | `crates/kk-service/tests/flow.rs` (sv261) |
| A review's findings are handed to other experts only while their model is on this computer or in confidential hardware whose proof is checked, never to a hosted model without that proof or to the outside coding agent, and a run or conversation is never continued with a less private model than the one it worked with | `crates/kk-service/src/service/team.rs` | `crates/kk-service/tests/flow.rs` (sv266 to sv270, sv297) |
| A skill pack is taken only from a publisher you pinned, and through a catalog only when it is the exact file listed | `crates/kk-packs/src/lib.rs`, `crates/kk-packs/src/catalog.rs` | `crates/kk-service/tests/flow.rs` (sv216, sv265), `tests/e2e/packs_web.mjs` |
| NEAR's verifier, not ours, vouches for a model key on chain | `contracts/kakurega-registry` | `contracts/kakurega-registry/tests/sandbox.rs` on a local node, and the testnet transaction above |
| The signed log shows what left this computer, and it can be verified anywhere | `crates/kk-eventlog`, `crates/kk-service/src/service/outbound.rs` | `crates/kk-service/tests/flow.rs`, `kaku log verify` |

Each component has a record of what was read, decided and tested, and what is not covered, in `docs/engineering-decisions/`.

</details>
