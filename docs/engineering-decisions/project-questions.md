# Questions about a project, answered by an agent that may only read and check

Ledger rows: E-A7 (subagents: a worker that answers a free question), G-GH-33 in part (experts that use tools themselves).

## Target experience
1. The owner asks a plain question about one of their projects ("does logout revoke the session token?", "where is the payment amount checked?"). An agent explores the project, as long as the question needs, and answers in plain words.
2. Every sentence the answer rests on points at a file and a line, and the program checks that the cited text is really there. What was not found is marked, never hidden.
3. Where running a little code settles the question ("what does `parse_amount('-5')` return?"), the agent writes a short script and the program runs it in the sealed sandbox on the project's files; the owner sees the script and what it printed.
4. Keys and private files are withheld exactly as in a review. Nothing is written, nothing leaves, the answer stays on this computer.
5. The same thing works from the web page, the terminal workspace and `kaku`, and a team run can give a task to it.

## References
| Reference (revision) | Read | What it does | Decision |
|---|---|---|---|
| OpenCode (fe04656, MIT) `packages/core/src/plugin/agent.ts` | FULL | The "Explore" sub-agent: read-only (read, grep, glob, read-only shell), `.env` files need a yes, a "quick / medium / very thorough" level chosen by the caller, the answer is its final message | ADAPT the read-only stance and the thoroughness levels. IMPROVE: its answer is unchecked prose, so a wrong path or an invented line is passed on as fact; here every citation is checked against the file |
| OpenCode `packages/core/src/tool/plugin/subagent.ts` | PART (1 to 200 of 311) | A child session with its own context; the result is the child's last message; follow-ups go to the same child | ADAPT: one question is one job with its own context. Follow-ups are a later step (EXCLUDE for now: a follow-up would read the earlier answer as context and needs the same level rules as chat) |
| Claude Code's Explore agent (closed) | docs only | Read-only search agent | Nothing beyond OpenCode's shape |
| Our coding mode (`crates/kk-review/src/code.rs`) | FULL for the workspace, the loop and `SandboxTester` | In-memory copy of the visible files, read and search tools, text-written calls recovered, repeated calls stopped, tests run in the sandbox | REUSE the workspace and its read tools, and the loop's guards; the agent gets no write tool |
| Our review checks (`crates/kk-review/src/check.rs` `run_reproducer`) | PART (486 to 700) | How a script is run in the sandbox on a copy of the files | REUSE the sandbox and limits; a check script is evidence shown with its output, not a verdict, so the three rounds of a reproducer do not apply |

## Approach
- `crates/kk-review/src/explore.rs`: the loop, the `answer` tool with citations checked when it is called (the agent is told which citations were not found and may correct them once), and `run_check` through a `ScriptRunner` that runs Python or JavaScript in the sandbox on the visible files only.
- The runner trait in the service gets `project_question`; the CLI runner implements it with the route in use and the sandbox; doubles say they cannot.
- Service: `POST /api/questions` (folder, question, depth), list, view, stop; only with a model on this computer, like a coding task (the agent reads the whole project; see Limits); one at a time; kept in the store; listed with the other work.
- Web: "Ask a project" in the menu and "Ask about its code" on each project; terminal `/explore`; `kaku explore`.

## Quality criteria
- A citation whose text is not at that line is never shown as checked (hostile test: a model that cites an invented line).
- A withheld file is never read, cited as checked or copied into a check (hostile test).
- A check script cannot reach the network or write the project (the sandbox), and its output is shown as output, not as the answer.
- Nothing about the question or the answer leaves this computer with the local model.

## Built
- `crates/kk-review/src/explore.rs`: the loop (read tools only, text-written calls recovered, the same call three times stopped), `answer` with each place checked on the spot (`check_citation`: the quote's lines in order, blank lines of the file between them allowed, at the cited line or the nearest other one; each quoted line of fewer than eight letters must be a whole line, and a quote made only of such lines must stand once; a copied line number or a cut-line mark is not held against it; a withheld file or a file that is not there is never checked, and a readable file is never taken for a withheld one by case), one correction round, `run_check` through `SandboxScripts` (the readable files copied read-only, copied again inside, Python isolated with the project after the standard library, 30 s, 15 s of CPU, no network). In a text answer only the lines its places were read from are taken out.
- `crates/kk-service/src/questions.rs`, `service/questions.rs`: `/api/questions` start, list, view, forget, stop; kept encrypted in the store; listed as work; one at a time; only with a model on this computer; a question that holds a key or a spoken password is refused.
- Web `#/explore` (Ask a project), a button on each project with a folder; terminal `/explore [quick|thorough] FOLDER : QUESTION`, `/explore show`; `kaku explore DIR QUESTION [--depth]`, `show`, `list`, `forget` (exit 0 when every line was found, 3 otherwise).
- Team runs: the Code reader seat (`project-reader`), only with the model on this computer; the run is held on this computer from its task on; its answer, and any answer written after it, is never offered to the coding agent outside.

## Found by using it with the real local model (qwen2.5-coder:7b, the sample project)
- First run: the model searched for the whole question as one phrase, found nothing, read no file and said it could not answer. Added: the files that hold the question's words are named in the first message (by name, then by text, with simple word endings dropped); a phrase that stands nowhere is answered with the lines that hold its words; a search also names files whose name holds the text; an answer given before any file was read is sent back once.
- Second run: the model answered correctly but wrote its lines as text ("path:15: def logout(...)", or labelled File/Line/Text). Those lines are now read out of the text and checked exactly like lines given through the tool.
- Third run: with only a search behind it, the model claimed logout revokes the token (wrong). A search no longer counts as having looked; only reading a file or a check that ran does.
- After these: both sample questions answered correctly, every cited line found, twice each (temperature 0). The screens say plainly that a found line shows the answer rests on real code, not that the answer is right.

## Evidence
kk-review explore ex01 to ex08; kk-service sv275 to sv278 (mutations: the found check, the withheld check, the short-quote rule, the local-model rule, the spoken-password refusal, the reader's tie to the review taint, its local-model rule and its hold mark each made a test fail); kk-tui `tu_ex01`; `explore_web.mjs` 18; `script_cli.py` 5 new; `tui_pty.py` 1 new; real model as above.

## Limits
- Whether the answer is right is not checked; only that the lines it cites exist. A script's output is evidence the owner reads, not a verdict.
- Changed since the lines above were written: a question, a coding task and the team's code reader now also run on NEAR AI Cloud's confidential hardware, by the review's rule (this computer or hardware whose proof is checked, never a hosted model without one, never a project kept on this computer, recorded in the signed log before a file is sent). Where this file says "only with a model on this computer" or "nothing leaves", read it with that change; see `near-route.md`.
- Follow-up questions in the same thread are not built.
- Text found at two places the same distance from the cited line is matched to the earlier one.
- With NEAR chosen for chat and a local model present, a question is refused, although the reader would use the local model: the stricter of the two checks applies.
- 100 questions are kept; the oldest answered ones make room, and the owner is told which.
