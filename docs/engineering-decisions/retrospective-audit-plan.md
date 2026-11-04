# Retrospective source-first parity and reuse audit: plan

Why: an independent audit found that several components were built on thin reading of their strongest references (terminal, web workspace, Telegram, MCP, memory) and that reuse was not maximised where licences allow. The standing rule (global CLAUDE.md, "PERMANENT RULE: OPPORTUNISTIC CAPABILITY DISCOVERY, MAXIMUM MEANINGFUL REUSE, AND RETROSPECTIVE AUDIT") applies to every component already built. The master goal and its gates are unchanged.

## Order (security first, then capability recovery, then the next milestone)
0. Security and correctness defects found by the audit, validated against the code (see `audit-validation.md`). Reproducer verification fixed first (done: negative control, CONTROL_OK, source-bound release text).
1. Capability extraction per component cluster (below), by independent investigation of local clones, each producing a matrix with file-and-line evidence.
2. I review every matrix personally, re-read the decision-critical code myself, and decide per capability: REUSE, ADAPT, IMPROVE, BUILD, REPLACE or EXCLUDE WITH REASON.
3. Implement the chosen capabilities in dependency order into the existing architecture (one kernel, one approval path), with end-to-end and hostile tests and, where behaviour matters, a run of the reference product on an equivalent task.
4. Record each result in the component's engineering record: sources and commits, exact files read and depth, discoveries, decisions, licence obligations, locations, tests, remaining gaps.
5. Resume the milestone sequence; gates stay honest.

## Clusters and local references (all under `private-ai/inspiration/` unless noted; commit and licence verified from the clones)
| Cluster | Kakurega components covered | References to study (commit, licence) |
|---|---|---|
| A Terminal and sessions | `kk-tui`, `kaku` CLI, session history, queueing, steering, branching, permissions UI, background tasks, notifications | opencode (fe04656, MIT) packages/tui and its server and session code; hermes-agent (29a61cb7, MIT) ui-tui, tui_gateway; codex (f2b94ef, Apache-2.0) codex-rs/tui and app-server; cline (e68a6dd, Apache-2.0) CLI; goose (71dd9a9, Apache-2.0) |
| B Web workspace | `apps/web`, projects, chat, search, source viewer, settings, onboarding, a11y | opencode packages/app, ui, session-ui; LibreChat (f4154b6, MIT); anything-llm (eda0eb5, MIT); OpenHands (01d1970, MIT) frontend; ClaraVerse (GPL-style notice: study only); Ghast screenshots and `_site` (no licence: ideas only) |
| C Channels | Telegram bridge and any chat channel | openclaw (f38495f7d, MIT) channels and gateway; hermes-agent gateway platforms (Telegram first); Ghast companion_service (MIT) |
| D MCP and agent access | `kaku mcp`, tool exposure, resources, prompts, long-running tasks, authorization | the official `modelcontextprotocol/rust-sdk` (to be fetched into `private-ai/inspiration`), MCP specification current version; opencode, goose, cline, hermes-agent MCP client and server code |
| E Memory, agent loop and coding | `kk-memory`, `kk-worker`, fixes, coding mode, context management, skills | mem0 (753496c, Apache-2.0); Anamnesis and ghast-memory-os (Apache-2.0); hermes-agent memory and skills; goose; openclaw memory; OpenHands microagents, condensers, runtime; cline edit and diff flow; opencode edit, diff and permission flow; codex apply_patch |
| F Desktop, extension, documents, verification and security tooling | desktop shell, extension, document parsing, secret and injection detection, evaluation | opencode desktop; cline; docling and anything-llm collector; gitleaks, detect-secrets, presidio, semgrep, promptfoo, agentdojo, camel-prompt-injection, open-code-review |

Independent discovery: each investigator must also search for strong references not listed here (competitors and open-source projects) and report them with the evidence for why they matter.

## Rules for the investigators
- Read the real source along execution paths; record the commit, the files and line ranges, and the reading depth. A README or file listing is not reading.
- Study the whole product, not only the feature that brought you there: every valuable capability found is a row in the matrix, with how it works, and whether Kakurega has, partly has, lacks or does worse.
- Licence: MIT, Apache and BSD may be adapted with notices kept; everything else is study-only. Never propose a second permission system or a second security boundary.
- Investigators make no code changes. They produce `capability-matrix/<cluster>.md` only, with a prioritised list of what to implement and in what order.

## Evidence required to close a cluster
Matrix reviewed by me; every REUSE, ADAPT, IMPROVE or BUILD decision implemented or scheduled with a reason; tests including hostile cases; a run of the strongest reference on an equivalent task where behaviour is the point; `NOTICE.md` updated for anything adapted; the owed list for the component empty or justified.
