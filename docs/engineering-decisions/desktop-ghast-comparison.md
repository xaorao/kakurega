# Desktop and web shell against Ghast (product gate, first pass)

Source of Ghast facts: `ARCHITECTURE/BENCHMARK_GHAST_WORKBENCH.md` and screenshots in `ARCHITECTURE/benchmarks/ghast/` (hands-on first run of Ghast 0.1.35 on this Mac; the app is no longer installed, only its data folder). Ghast is closed source, so nothing is copied; ideas only. Correction: an earlier reply here said the running Ghast app had not been opened; it had, for first run only. Task quality, long runs, recovery and Team/Coding flows were never exercised there (no funds).

Kakurega screens compared: `target/e2e-shots/desktop-01-workspace.png`, `desktop-02-approved.png`.

| Dimension | Ghast (SEEN) | Kakurega now (SEEN) | Verdict |
|---|---|---|---|
| First run | 5 skippable steps in about 2 min: language, invite, wallet, name, theme | Opens straight into the workspace, auto-paired, nothing to set up; no language, name or theme step; no check that Ollama and the sandbox VM are ready | Ghast ahead on onboarding polish; we win on zero steps but must add a readiness screen |
| Wall before value | Blocked by a 0G top-up | None: a local private model works with no payment | We win (their weakness, our opening) |
| Main window | Calm, one large composer, greeting, sidebar Chats/Coding/Team, Cmd+K search, Projects/Recent | Task form (folder, recipient), Reviews list, tabs; no search, no history grouping, no greeting | Ghast ahead on feel and navigation |
| Privacy visibility | Permission shield; default "Full access"; unencrypted local DB; analytics | Route badge "Private: model runs on this computer", withheld secrets, approval of exact files | We win |
| Approvals | In-app prompts | Separate native window with Touch ID, exact bytes and digest | We win on strength; ours is only one flow |
| Memory/size | 1.2 GB on disk | 253 MB app, about 450 MB RAM | Ours smaller (Ghast memory not measured) |
| Breadth | Plugins store 100+, Team, Coding, 11 languages, voice | One workflow (review a project) | Ghast far ahead; breadth is Milestone II to III work |

## Adopt next (meaningful, in order)
1. Readiness screen on first launch (Ollama reachable, model present, sandbox ready, with one-click fixes) so a stranger reaches the first review without help; ties to I-07.
2. Persistent history with search (Cmd+K) over past reviews.
3. Theme choice (system, light, dark) in the UI; tokens already support both.
4. Greeting line that frames the task ("start with something that can be clearly finished").
5. Localisation scaffolding (strings out of components) before adding languages.
6. From OpenCode (MIT): single-instance lock, early window before heavy load, renderer served from a private scheme, port-per-window IPC when we need IPC. Not adopted yet.

## Not covered
Ghast Team/Coding/Plugins behaviour, voice, speed of real tasks, error recovery; no side-by-side run is possible now because Ghast is not installed (reinstall from its public download if a fresh run is needed).
