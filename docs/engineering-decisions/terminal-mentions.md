# Terminal @file references (ledger A-A17, B-W29)

## Target experience
1. In the terminal workspace the owner types `@` and the start of a file name, presses Tab, and the name is finished (a folder gets a slash, a file gets a space). Sending the message reads each named file on this computer, attaches it the way the web page does (offline parser, labelled, passages that read like instructions or hold a secret left out), and the answer says how many attached files it read. A file that cannot be attached stops the message and gives it back, with the reason.
2. Strongest reference: OpenCode's prompt autocomplete (`@` files, folders, agents and references; fuzzy search by the server; a file becomes a structured part of the prompt).
3. Approach: the terminal names the file, the service reads it. Nothing is read by the model and nothing is inlined by the terminal.
4. Quality criteria: only files under the folders the service may read; links and `..` that leave them are refused; secret-named files never attach; one pipeline for uploads and paths; a bad name changes nothing.

## Reference and reading depth
- OpenCode (MIT) fe04656 `packages/tui/src/component/prompt/autocomplete.tsx`: lines 90 to 720 read, plus 820 to 870 (the trigger and its reopening); `prompt/display.ts` `mentionTriggerIndex` and `slashTriggerIndex` read. NOT READ: lines 1 to 90 and 720 to 820 and 870 to 1023 (key bindings, rendering of the popup, the footer prompt), `prompt/frecency`, `client.api.file.find` (the server's fuzzy file search).
- What it does: a mention starts at the last `@` that follows whitespace or the start and has no whitespace after it; the server returns up to 20 fuzzy matches for the typed part, and a file may carry a line range `path#10-20`; a selected file becomes a structured file part (uri, name, mention span) rather than text; choosing again moves the existing mention; a folder is expanded with a slash and searched again; agents, skills and named references are offered in the same list; matches are ordered by fuzzy score with a frecency boost; the list shows "Searching...", "Could not search files. Keep typing to try again." and "No matching files, agents, or references".

## Decisions
| Capability | Decision | Reason |
|---|---|---|
| `@` trigger rule (after whitespace, no whitespace after) | ADAPT | Same rule, so an address like `x@y` is never a mention |
| File completion | ADAPT, simpler: Tab completes by prefix from the service's folder listing (`/api/fs?files=1`), as `/review` folders already do | A popup with fuzzy ranking needs the popup widget; prefix completion is the terminal's existing idiom |
| File as a structured part, not inlined text | ADAPT: the file goes through the attachment pipeline and its id travels with the message | Labelling and secret handling already live there |
| Reading by the model's tool | EXCLUDE | Chat has no tools by design |
| Line ranges `path#10-20` | ADAPT, built: `@file#10` or `@file#10-20` attaches only those lines (the file is read whole by the service, the lines are cut before parsing, the attachment is named `name.lines10-20.ext`) | OpenCode puts the range in the file part's uri; here it travels as a field so the service validates it
| Agents, skills, references in the same list | EXCLUDE | No such objects in the terminal chat |
| Frecency boost | OWED (value unproven) | |
| Fuzzy ranking | OWED with the popup | |

## Beating the source (its weaknesses and ours)
- OpenCode's file search takes any path the server can reach after its own permission check; here the service resolves links first and refuses anything outside the folders it was started with, and refuses names that say they hold secrets (`.env`, `*.pem`, `*.key`, `id_rsa*`, `credentials*` and so on) even inside them, in the listing and in the attach call.
- A file that cannot be attached stops the whole message and puts it back in the box; the source would send the text with a part missing.

## Evidence
- Service: `kk-service` flow sv179 (401 without a session; inside the roots attaches and the text reaches the model; outside, a link out, a traversal, a folder, a missing file, `/etc/hosts` and an empty path are refused with `NOT_ALLOWED`; three secret names are refused with `SECRET_FILE`; an empty file, a 9 MB file and an unknown field are refused; the listing hides hidden and secret-named files and only lists files when asked). 6 of 6 mutants caught.
- Terminal: tu_at01 (token rules), tu_at02 (attach first, then send the ids; queued message follows; footer lines), tu_at03 (a refusal stops the message, puts it back, drops the queue), tu_at04 and tu_at05 (Tab completion and its refusals); 8 of 8 mutants of the mention logic and 8 of 8 of the completion caught.
- Real use: `python3 tests/e2e/tui_mentions.py` drives the real binary in a pseudo-terminal with a real qwen2.5:3b: Tab finishes `@terms.txt`, the file is attached, the answer is from it (net 45), a `.env` mention is refused with its reason and the message returns, `@/etc/hosts` is refused, and the token value never reaches the screen (9 checks; two of its first checks passed falsely by matching earlier text and were rewritten to read the outcome).

## Line ranges
- Evidence: sv182 (only the chosen lines reach the model; names; clamping; ten refused specs; secret name refused with lines; non-text refused; 7 of 7 mutants caught), tu_at06 (parsing, request body, refusal; 5 mutants caught, one redundant guard removed after a mutant of it survived), `python3 tests/e2e/tui_mentions_lines.py` 5 of 5 with a real qwen2.5:3b (mango and plum from lines 3 and 4, none of the other fruits, a line past the end refused with the real length).

## Not covered
- The cwd of the terminal must lie inside the folders the service was started with, or Tab finds nothing and a mention is refused; the message says so.
- The web chat attaches by upload and drop, not `@`.
- Frecency and fuzzy order (owed above). Lines can be chosen only in text files (UTF-8); up to 2000 lines.
