# Audit 1: an independent reviewer attacked the components built after the NEAR route (state: findings validated and fixed; mutation results below)

Method: one fresh reviewer agent read the code (kk-methods, kk-packs and its service, kk-mcpc and its service and the coding-loop path, method suggestions, the broker's text-call recovery, the registry contract) with attacker capabilities (malicious project, malicious MCP server, malicious pack publisher, prompt-injected model, contract caller). I validated each finding against the real code before accepting it. Two whole-surface tests were added by me (sv221: every API path refuses an anonymous caller; sv222: every write path refuses a session without its CSRF token).

| # | Finding (reviewer's severity) | Verdict | Fix | Evidence |
|---|---|---|---|---|
| 1 | `ask` showed only the first 1000 characters; the call ran with all of it (high) | CONFIRMED in `mcp.rs` | the owner sees the whole call (cleaned); one over 1500 characters is refused without asking | sv223 |
| 2 | An outside tool's error text skipped the untrusted frame and the cut (medium) | CONFIRMED in `code.rs` | errors are framed and cut at 1500 characters | lp20 |
| 3 | A quoted `<tool_call>` inside long prose could run an outside tool (medium) | CONFIRMED by reading `calls_in_text` | a call to an `mcp__` tool is taken only from a reply of at most 600 characters; workspace tools are unchanged | tc_outside_tools (kk-route) |
| 4 | Invisible characters (tag characters, U+2028, U+3164, U+2800, U+FE0F, soft hyphen, mongolian separator) passed in methods and in outside-tool text (medium) | CONFIRMED (reviewer's probe) | one extended set in kk-methods and kk-mcpc | mt19, mc17 |
| 5 | The owner never saw the input schema the model sees; its strings were not cleaned (medium) | CONFIRMED | cleaned recursively; returned by `/api/mcp`; shown in the web screen | mc19, sv224 |
| 6 | No page cap on a server's tool list (medium) | CONFIRMED in rmcp's `list_all_tools` loop | our own paging with a 20-page cap and the tool cap checked per page | mc18, mc22 |
| 7 | Registry squatting: entries keyed by digest alone, so anyone could register a capsule's digest first with their own key and make-believe receipts (medium) | CONFIRMED by reading `register` | entries are keyed by digest plus signer; the binding signature now covers a digest of the receipts | rg05b, rg05c, sandbox run on a real runtime, 3 mutants |
| low | `suggest_method` with no triggers sent null and was dropped | CONFIRMED | null reads as none | mt20 |
| low | everyday words as triggers fire on everything | CONFIRMED | stop-word list added to the common words | mt20 |
| low | method names case-sensitive in storage, matched case-insensitively | CONFIRMED | unique whatever the case, for saves, suggestions and packs | sv226 |
| low | pack versions were tracked by pack name only (one publisher could block another) | CONFIRMED | tracked per name and publisher key | sv227 |
| low | tool names with `__` could collide | CONFIRMED | such tools are left out | mc21 |
| low | server started in the service's working folder; program not pinned by hash | CONFIRMED | own working folder; SHA-256 of the program taken when added and checked at every start | mc20, sv225 |
| low | pack import is not one database transaction | NOT FIXED: a single owner session; recorded as owed | | |
| design | the read-only claim comes from the server | ACCEPTED, documented and labelled "the server says" | | |

Not covered by this audit (reviewer): did not run the tests; did not measure memory under the page flood; could not check whether the registry is deployed without keys (it is not deployed). The reviewer found no policy bypass in the broker's text-call recovery (a recovered call still goes through `op_tool`).

Owed: the terminal does not yet show a tool's input schema (web does); a real attack run of a malicious MCP server against a real model; the pack import transaction.
