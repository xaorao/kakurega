# Skills, plugins and the catalog (ledger G-GH-55, G-GH-58, G-GH-59, G-GH-62)

## Target experience
1. The owner opens a shelf of ready-made skills, adds the ones they want, reads each and approves it; later they install more from a publisher's signed catalog, and a plugin can bring skills and suggest outside tools. Installing never gives anything power: a skill is text that applies after approval, a tool is off until the owner allows it.
2. Strongest references: Hermes (skills hub: quarantine, scan, lock file, update checks bound to the recorded source), OpenCode (skills loaded on demand), goose (plugins and hooks), Ghast (a 496-entry plugin store and 30 bundled skills, as the experience benchmark; its code is closed).
3. Approach: build on what exists here. A skill is a method (`kk-methods`): text and trigger words, with approval pinned to a hash. A signed pack (`kk-packs`) already carries methods from a pinned publisher with expiry and rollback protection. Added on top: bundled methods, then a signed catalog, then packs that may suggest outside tools.
4. Quality criteria: nothing applies before the owner approved its exact text; adding or importing never replaces a method the owner has; a catalog or pack that is unsigned, altered, expired or older than one already seen is refused whole; nothing in a pack can name a tool, a route, a secret or a permission.

## What was read (by me)
| Source (commit) | Files | Depth | What it settled |
|---|---|---|---|
| Hermes (29a61cb7, MIT) | `tools/skills_hub_install.py` | FULL | quarantine then move; a lock file with source, trust level, scan verdict and content hash; uninstall and update resolve paths without following links; an update is fetched only from the source it was installed from. Weak points: the old copy is removed before the new one is moved in; the content hash is cut to 16 hex characters; a 100,000-character skill only gets a warning |
| Hermes | `tools/skills_guard.py` 18-66, 154-175, 820-838, 939-962 | PART | trust levels by source and a verdict table; the scanner looks for shell exfiltration because their skills carry scripts that an agent runs |
| Mapping report (subagent, REPORTED) | `private-ai/specs/maps/plugins-skills-map.md` | n/a | where the rest lives |
NOT READ yet: goose hooks and plugins; OpenCode skill loading; openclaw install trust; the OAuth sources (rmcp `auth.rs`, goose `oauth`, OpenCode `mcp/oauth.ts`).

## Decisions
| Capability | Decision | Reason |
|---|---|---|
| Bundled skills | BUILD: 18 methods written here, shipped in the binary, listed by group; added unapproved, approved by the owner like any method | Ghast's 30 are tool skills and closed; ours are guidance texts, which is what a method is. One rule for every method, shipped or not |
| Skill scanning | EXCLUDE the shell-pattern scanner | a method here holds no script and names no tool, so there is nothing for it to find; the risk that remains is persuasive text, which the owner reads before approving and which reaches the model inside a frame it cannot close |
| Install paths, quarantine, lock file | EXCLUDE | methods live in the encrypted store, not in folders, so the path-escape class does not exist; the store row with its hash and approval is the lock |
| Picking a skill by name in a message | exists (`/name`) | |
| Signed catalog, packs that suggest tools | OWED, next | |

## Built: bundled skills
- `crates/kk-methods/src/bundled.json` and `bundled.rs` (18 methods in five groups: code review, threat model, bug report, test plan, commit message, incident review, SQL review, API design, dependency check, release notes, meeting notes, decision record, contract reading, privacy check, plain language, email reply, numbers check, research brief).
- `GET /api/methods` lists them with whether the owner already has one of that name; `POST /api/methods/bundled` adds one, unapproved, and never over an existing name. Web: a shelf on the Methods screen. Terminal: `/method bundled`, `/method take NAME`.
- Evidence: kk-methods bd01 (each is a valid method, unique, bounded, with no order to ignore rules and no address) and bd02 (each applies to the request it is for, none to an unrelated question, "smart contract" does not bring the legal one); kk-service sv247 (on the shelf it does not apply; added it does not apply; approved it does; nothing is replaced); kk-tui tu116; `tests/e2e/bundled_web.mjs` 9 browser checks, screenshots read by me.
- Limit: whether a given model follows a skill's guidance well is a quality question for the model; with the 3b local model it often does not.
