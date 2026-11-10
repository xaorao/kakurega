# Project settings: a project that answers only from its own material (ledger B-W05b, G-GH-06)

## Target experience
1. The owner names a project in Chat; its notes and instructions are offered and no other project's. The owner can also say "this project answers only from its own notes and attached files": a question that nothing there fits gets a plain "nothing here" and no model is asked at all; a question that something fits is answered from it, and the model is told to use nothing else.
2. Strongest reference: AnythingLLM workspaces (chat mode "query" with a refusal text, per-workspace prompt, model, history length, similarity threshold, top-N).
3. Approach: one owner-only switch per project, stored in the encrypted store; the kernel decides, not the model.
4. Quality criteria: no model call and no usage entry when nothing fits; the reply says no model was asked; attached files count as material; a standing instruction does not; other projects and project-less chats are unchanged.

## Reference and reading depth
- AnythingLLM (MIT) eda0eb5 `server/models/workspace.js`: 726 of 726 lines, FULL. What a workspace holds: name and slug, prompt (with a change history), temperature, history length, similarity threshold, top-N, chat mode (`chat`, `query`, `automatic`), provider and model, agent provider and model, a refusal text for query mode, vector search mode. NOT READ: `server/utils/chats` (where query mode refuses), `workspaceThread.js`, the prisma schema beyond `workspaces`.

## Decisions
| Capability | Decision | Reason |
|---|---|---|
| Query mode (answer only from the workspace's documents, refuse otherwise) | ADAPT, rebuilt: `notes_only` per project; the refusal needs no model call | A grounded mode is a privacy and trust feature. The refusal is decided by what was recalled, not by the model |
| Per-workspace prompt | KEPT: project instructions already exist (`owner_instructions`) | |
| Prompt change history | EXCLUDE for now | Instructions are short and the owner wrote them; revisit with the owner-instructions component |
| Per-workspace model and provider | OWED, after the NEAR route exists | Only a local route exists today, so a "stays on this computer" project setting cannot be tested yet; it will use `route_ceiling` |
| Similarity threshold, top-N, history length per workspace | EXCLUDE | Recall already chooses by a measured cut-off; per-project knobs add settings nobody can judge |
| Workspace users and roles | EXCLUDE | Single owner product |

## Evidence
- `kk-service` flow sv178 (fails on the old code: no setting, the model is always asked): 401 without a session; six bad bodies refused; off by default; on means no model call, route "none", fixed reply, no usage entry; other projects and no project unchanged; an instruction alone is not material; a note makes the model get asked with the rule as a system message; an unrelated question gets nothing again; an attached file counts; a kept chat stores the fixed reply; the streamed chat takes the same path; off again restores the model call.
- Mutants 7 of 7 caught (gate never fires, fires with a note, usage counted, rule not sent, project ignored, off does not remove, files ignored).
- Browser (`tests/e2e/notes_only.mjs`, real qwen2.5:3b): switch on in Instructions and still on after a reload, nothing saved gives the plain reply with the footer "No model was asked", a saved note gives the answer from it, switch off, zero console errors. The screenshots found three defects, all fixed: the footer said a model answered and counted tokens, the project card vanished before it could confirm turning off, and a new project showed twice.

## Not covered
- Web pages chosen for a message count as material (`web_used`) but no test fetches one in this mode.
- The terminal app and `kaku` have no switch yet (the service has it); the Hindi texts are unread by a person.
