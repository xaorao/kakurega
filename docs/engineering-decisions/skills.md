# Skills and methods: owner-approved instruction packages (ledger G-GH-54 to G-GH-56, G-GH-59 to G-GH-62, E-A9, E-M24; status: methods built on service, web and terminal; suggestions from reviews built; plugins, signed catalog and bundled skills not built)

## Target experience
1. The complete experience: the owner keeps small named instruction texts ("methods": how I want a review written, a checklist for contracts, a house style) that the assistant can use in a conversation or a review. Nothing becomes active until the owner approves that exact text. Editing a method makes the old approval void. A method can only narrow what the assistant does or add guidance; it never grants a tool, a route or a secret.
2. Strongest references: IronClaw `crates/domains/ironclaw_skills` (Apache-2.0, b0b999d), Hermes skills (MIT), OpenCode `.opencode/skills` (MIT), Ghast skills and plugin store (closed docs, study only).
3. Approach: a method is content-addressed text plus metadata in the vault, versioned; approval is pinned to the content hash; the text reaches a model only as labelled untrusted guidance inside the already-existing "notes are data" boundary; selection is deterministic and bounded.
4. Quality criteria: a forged name, version or tag cannot break the prompt frame; an edited method is inactive until re-approved; a method cannot name or enable a tool; routing metadata that fires on everything is refused; every limit is enforced and tested with hostile input.

## References and reading depth
- IronClaw b0b999d (Apache-2.0), `crates/domains/ironclaw_skills/src/types.rs` lines 1-330 of 809 (PART: trust enum, activation criteria with caps, manifest, gating, credential specs, loaded skill with a content hash); `validation.rs` lines 1-330 of 816 (PART: name and version grammars, tag-breakout escaping, safe relative paths, line-ending normalisation before hashing, the routing-metadata lint). `parser.rs` lines 1-260 of 459 (FULL for the parse path; the rest is tests); `selector.rs` lines 90-490 of 1531 (FULL for cost, selection, chain-loading, scoring, mention extraction). NOT READ: `management.rs`, `learning.rs`, `scoped_management.rs`, the loop-host activation code (5021 lines), the install and URL bundle code. Also read: its `.claude/rules/skills.md` (the trust table, selection pipeline, truncation caps).

## What the reading showed (OBSERVED in the code)
| Finding | Decision |
|---|---|
| Name grammar `^[a-zA-Z0-9][a-zA-Z0-9._-]{0,63}$` and version grammar `^[a-zA-Z0-9._\-+~]{1,32}$` exist because the version was once interpolated into a prompt attribute and `1.0" trust="TRUSTED` forged a higher trust | ADAPT the grammars; our frame must not interpolate any metadata into attributes at all |
| Content is escaped so `<skill`, `</skill`, `< skill`, NUL-padded and mixed-case forms cannot close or open the frame | ADAPT: neutralise our own frame markers in method text, case-insensitively, with whitespace and control characters between |
| Keyword, pattern and tag caps with a minimum length of 3; `exclude_keywords` veto | ADAPT: caps and minimum length; no regex patterns (not needed, a second failure mode) |
| A lint refuses routing words that fire on everything (measured: fixing the catalog cut false activations by 68% where boundary matching cut 18%) | ADAPT the idea: refuse a method whose trigger words are all too common; our list is rebuilt from our own use, not copied |
| Hash computed over line-ending-normalised content | ADAPT: approval pins the normalised hash |
| Installed (registry) skills expose only a description, user-placed ones their body | EXCLUDE the registry tier: no remote install source. Every method is owner-placed; imported text is a draft until approved |
| Tool access is decided elsewhere, never by the skill | Same rule here: a method carries no tool, route or credential field at all; a field of that name is a refusal |

## More findings from the parser and selector
| Finding | Decision |
|---|---|
| The parser silently drops unknown frontmatter fields (its own warning admits a skill author can think gating works while it is inert) | IMPROVE: unknown fields are refused, never ignored; a `tools`, `route`, `credentials` or `allow` field is a named refusal |
| YAML frontmatter needs a YAML parser | EXCLUDE: methods are structured fields in the vault, edited by a form; text import accepts only a strict three-field header |
| Substring keyword matches score 5; fixing the catalog beat boundary matching | ADAPT: whole-word match only, caps and minimum length |
| Declared `max_context_tokens` can understate; the code falls back when the estimate is 2x higher | IMPROVE: cost is computed from the real text only; nothing declared is trusted |
| `/name` mention rewrites the user's message, putting the skill's description into it | EXCLUDE: an explicit mention selects the method but never edits the owner's message |
| Chain-loaded companions (depth 1) ride on the parent's score | EXCLUDE: no companions; one method is one text |
| Regex activation | EXCLUDE (second failure mode, needs its own size and time bounds) |
| Name-and-description fallback only when no criteria scored (agent-written skills had no activation block: 0 of 30) | ADAPT for methods with no trigger words |
| Confidence factor halves the score of learned skills | ADAPT later with learned methods (proposals) |

## Owed before building
1. DONE: parser and selector read. NOT READ still: `management.rs`, `learning.rs`, the loop-host activation.
2. Read Hermes `tools/skills_tool.py` and the skill manager, and the OpenCode skill loader, for the user-facing flow (list, view, edit, approve).
3. Write the capability matrix and the behaviour spec, then tests that fail first.
4. Decide where the store lives (the vault beside notes) and how the three surfaces (service, web, terminal) reach it.

## Built
- `kk-methods` (hash-pinned approval, whole-word selection, a frame the text cannot close, bounds), service endpoints, chat use, web and terminal screens. Evidence: mt01 to mt18 (16 of 16 mutants), sv190 to sv193 (7 mutants, 2 equivalent), tu_me01 to tu_me03, `tests/e2e/methods_web.mjs` (a real model followed an approved method once, observed).
- Suggestions from reviews (G-GH-54 as proposals): a review may call `suggest_method` (at most 2); the suggestion arrives as a proposal (label untrusted), is dropped if invalid or holding a secret, at most 20 wait, keeping one saves an UNAPPROVED method and never replaces an existing name. Evidence: wf10, j27 (VM), sv213 to sv215 (6 of 6 mutants), tu_me04 (4 of 4). OBSERVED once on this Mac: a real review of the synthetic repository by qwen2.5-coder:7b produced no method proposal (it also found no defect, see TESTING.md), so a small local model does not use the tool; whether a larger model would, and whether its suggestions are any good, is NOT VERIFIED.
- Not built: bundled skills (G-GH-55), plugin packages and the signed catalog (G-GH-58, G-GH-59, G-GH-62), declarative hooks (G-GH-61), methods in the review sandbox (excluded on purpose), methods in coding tasks.

## Signed method packs (owner chose this as the next build)
- Target: a publisher (a person or team the owner trusts) signs a pack of methods; the owner pins the publisher's key once, imports a pack file, and every method arrives saved but UNAPPROVED. A pack never carries a tool, route, credential or permission (a method has no such field), never replaces an existing method, and cannot be replayed at an older version.
- Reading: IronClaw `management/install_bundle.rs` 1-140 of 672 (PART): bounded bundles (at most 256 files, 2 MiB per file, 20 MiB total), a path grammar that refuses traversal, symlinks refused. Our own `kk-update` (`check`, DSSE with a pinned root, rollback by highest seen version, expiry) READ by me in the update work. NOT READ: the rest of IronClaw's installer, its URL and GitHub bundle sources, Ghast's catalog format (closed docs).
- Decisions: REUSE our DSSE envelope and pinned-key check (one signature, strict verify, canonical payload); EXCLUDE zip or tar bundles and URL or GitHub sources (a pack is one JSON file the owner brings, so no archive bombs, no path traversal, no network); EXCLUDE a root-of-trust with rotation for now (the owner pins publisher keys directly and can remove one; rotation can reuse `kk-update::root` later); IMPROVE on IronClaw's per-skill trust tiers: there are none, a method is never more trusted than text the owner approved.
- Limits: a pack is at most 256 KiB, 20 methods, one signature; names unique inside the pack and not already saved; `not_after` required; `pack_version` must not be lower than the highest seen for that pack name.

### Built: signed packs
- `kk-packs` (layer 3): `check_pack` (size, one DSSE signature by a pinned key, strict shape, every method through `kk-methods`, expiry, rollback by pack name) and `sign_pack` (refuses what a recipient would refuse); service `/api/packs*` (up to 10 pinned publishers, weak keys refused, import all-or-nothing, never replaces a method, secret scan, recorded highest version per pack name); `kaku pack keygen|sign|check`; web screen under Instructions. Evidence: pk01 to pk10 (12 of 12 mutants), sv216 to sv220 (11 mutants, after an invalid first run and tests that did not exercise the label rules were fixed), pc01 and pc02 (3 mutants), `tests/e2e/packs_web.mjs` (9 of 9 in a real browser, screenshot read).
- Not built: pack rotation of the publisher key, a catalog or index of packs, packs of outside-tool configurations (excluded on purpose: a pack must never carry a program to run), importing from a URL. The Instructions page now holds five long sections and needs a contents list in the polish phase.

## Bundled skills, catalog and plugins (added; ledger G-GH-55, G-GH-58, G-GH-59, G-GH-62)
Target for this part: the owner opens a shelf of ready-made skills, adds the ones they want, reads each and approves it; later they install more from a publisher's signed catalog, and a plugin can bring skills and suggest outside tools. Installing never gives anything power.

### What was read for this part (by me)
| Source (commit) | Files | Depth | What it settled |
|---|---|---|---|
| Hermes (29a61cb7, MIT) | `tools/skills_hub_install.py` | FULL | quarantine then move; a lock file with source, trust level, scan verdict and content hash; uninstall and update resolve paths without following links; an update is fetched only from the source it was installed from. Weak points: the old copy is removed before the new one is moved in; the content hash is cut to 16 hex characters; a 100,000-character skill only gets a warning |
| Hermes | `tools/skills_guard.py` 18-66, 154-175, 820-838, 939-962 | PART | trust levels by source and a verdict table; the scanner looks for shell exfiltration because their skills carry scripts that an agent runs |
| Mapping report (subagent, REPORTED) | `private-ai/specs/maps/plugins-skills-map.md` | n/a | where the rest lives |
NOT READ yet: goose hooks and plugins; OpenCode skill loading; openclaw install trust; the OAuth sources (rmcp `auth.rs`, goose `oauth`, OpenCode `mcp/oauth.ts`).

### Decisions for this part
| Capability | Decision | Reason |
|---|---|---|
| Bundled skills | BUILD: 18 methods written here, shipped in the binary, listed by group; added unapproved, approved by the owner like any method | Ghast's 30 are tool skills and closed; ours are guidance texts, which is what a method is. One rule for every method, shipped or not |
| Skill scanning | EXCLUDE the shell-pattern scanner | a method here holds no script and names no tool, so there is nothing for it to find; the risk that remains is persuasive text, which the owner reads before approving and which reaches the model inside a frame it cannot close |
| Install paths, quarantine, lock file | EXCLUDE | methods live in the encrypted store, not in folders, so the path-escape class does not exist; the store row with its hash and approval is the lock |
| Picking a skill by name in a message | exists (`/name`) | |
| Signed catalog | BUILT | see "Signed catalog" below |
| Packs that suggest tools (`plugin.json`, `mcp.json`) | BUILT (ADAPT): a signed pack may name up to five outside programs it suggests | see "Packs that suggest outside programs" below |

### Built: bundled skills
- `crates/kk-methods/src/bundled.json` and `bundled.rs` (18 methods in five groups: code review, threat model, bug report, test plan, commit message, incident review, SQL review, API design, dependency check, release notes, meeting notes, decision record, contract reading, privacy check, plain language, email reply, numbers check, research brief).
- `GET /api/methods` lists them with whether the owner already has one of that name; `POST /api/methods/bundled` adds one, unapproved, and never over an existing name. Web: a shelf on the Methods screen. Terminal: `/method bundled`, `/method take NAME`.
- Evidence: kk-methods bd01 (each is a valid method, unique, bounded, with no order to ignore rules and no address) and bd02 (each applies to the request it is for, none to an unrelated question, "smart contract" does not bring the legal one); kk-service sv247 (on the shelf it does not apply; added it does not apply; approved it does; nothing is replaced); kk-tui tu116; `tests/e2e/bundled_web.mjs` 9 browser checks, screenshots read by me.
- Limit: whether a given model follows a skill's guidance well is a quality question for the model; with the 3b local model it often does not.
- Learn a skill from a conversation: built. In a kept conversation "Suggest a skill from this conversation" asks the model (the same one, through the same hiding layer the chat uses) to write the way of working the owner taught as one method. The answer is treated like a review's suggestion: it must read as a valid method, name no power, hold no key or password (spoken ones included), and it only becomes a suggestion under Skills, marked "From a conversation". Nothing uses it until the owner keeps it and then approves it. Phrases that do not fit the limits for trigger and avoid words are left out, not a reason to drop the whole suggestion (found by the real check: the first real answer was dropped for a long avoid phrase). Evidence: kk-service sv262; one real run with a hosted Claude model on the outside test route (a conversation that set a release-notes format gave a usable method; small talk gave none). NOT TESTED: the button in a browser; a small local model's answers.

## Signed catalog (built)
- What it is: a publisher's signed list of their packs (name, version, one-line description, the methods each holds, the file's name and its SHA-256), checked against the same pinned key as a pack. It carries no method text and gives no power. `crates/kk-packs/src/catalog.rs`.
- What the checks are for: a changed list is refused (signature); a list past its expiry is refused, so an old list cannot be shown for ever to hide a newer one; a list issued before the newest one already opened from the same publisher is refused, so a withdrawn pack cannot be brought back by replaying an earlier list. A pack added through a catalog must be the exact bytes the catalog lists, under the name, version and publisher it lists; it is then checked whole as a pack, and every method still arrives unapproved.
- Publisher: `kaku pack catalog --key FILE --dir FOLDER --out FILE [--days N]` (every `.kkp` in the folder must be a pack this key signed) and `kaku pack check-catalog --catalog FILE --trusted KEY [--dir FOLDER]`.
- Owner: Skills, "A publisher's catalog": open the catalog file, see each pack with "Not added", "Added" or "Newer than your X", and add one by choosing its file. Nothing is downloaded by the app.
- Evidence: kk-packs ct01 to ct04; kk-service sv265 (unpinned, a same-publisher pack the list does not hold, a digest listed under another name, an earlier list, an expired list, another publisher's list, the update state); `packs_web.mjs` 15 (6 new, with the real commands and a real browser).
- Limits: the app fetches nothing, so the owner gets the catalog and the pack files themselves (a folder, a download, a shared drive). The freshness check is per installation: a first-time reader can be shown any unexpired list. A catalog does not say a pack is good, only that its publisher lists it.
- Design choice, mine: no network fetch of catalogs. A fetch would be one more thing leaving this computer on a schedule; the owner's own download keeps "what left this computer" unchanged.
- A catalog lists, it does not revoke: a pack its publisher took off the list still imports from its own signed file until it expires. A catalog dated more than ten minutes ahead of this computer's clock is refused.

## Packs that suggest outside programs (built)
- Read for it: Ghast's plugin store (`private-ai/ghast-ai/ghast-plugins`, b394a78, no licence: ideas only), `plugins/langfuse/plugin.json` and `mcp.json` in full, and the listing of the other 495 plugin folders. A plugin there is a manifest, an `mcp.json` that names tool servers (most of them remote, over HTTP), and skills; a credential is referenced as `$VAULT:name` inside the manifest and filled by the app. NOT READ: Claude Code's plugin format, the agent-plugins.org schema text.
- Decision: ADAPT the idea that a pack can name the tool programs its skills work with. Kept from ours: the pack is signed by a pinned publisher and checked as before. EXCLUDED: installing or starting anything from a pack, credentials or their references in a pack (a suggestion names variables only, the owner fills the values), remote servers (our client runs local programs only), and any variable that changes what runs (PATH, LD_ and DYLD_ variables, NODE_OPTIONS, PYTHONPATH and the like).
- What it does: `servers` in a pack (at most five), each a plain name, a full path with no `..`, up to 32 plain arguments, up to 16 variable names and a short reason. A pack whose suggestion breaks any rule, or whose arguments hold a key, is refused whole. Imported, a suggestion is listed under Outside tools with its exact command line and the variables it needs; "Fill the form with it" puts it into the usual add form, where the owner checks the path, fills in the values and adds it; each of its tools then stays off until allowed, as for any program. Dismissed, it is not shown again until the pack is imported again; once a program of that name is added, it is not shown.
- Evidence: kk-packs pk11, pk12 (16 refused shapes); kk-service sv279; `packs_web.mjs` 20 (4 new). Mutations, each made a test fail: the full-path rule, the forbidden variables, the `..` rule, the key scan over suggestions, hiding a suggestion once added.

## Earlier texts of a skill (built)
- Every text a method has had is kept in the encrypted store with when it was saved and whether the owner approved it then; at most twenty per method; removing the method removes them all.
- Brought back, an earlier text becomes the method's text and waits for approval again, even if that exact text was approved before: an approval is of the text the method has now. Bringing back the text it has now is refused, so an approval is never taken away for nothing.
- Web: "Earlier texts" under each method's text, with "Bring this text back". Terminal: `/method history NAME` lists them by number, `/method restore NAME NUMBER` names one of those (a number not listed is refused).
- Evidence: kk-service sv280 (mutations: the approval rule, removal of history, the limit of twenty each made it fail); kk-tui tu_mh01; `methods_web.mjs` 2 new.
