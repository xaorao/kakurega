# Kakurega MCP server

`kaku mcp` speaks the Model Context Protocol (version 2025-06-18) over standard input and output, one JSON message per line. It lets an agent start and watch reviews on your computer without ever seeing what is in them.

## Use it

The service must be running (`kaku serve`, or the desktop app, or the terminal workspace). Then add the server to an MCP client, for example:

```json
{ "mcpServers": { "kakurega": { "command": "kaku", "args": ["mcp"] } } }
```

To get the exact text for your client instead of writing it by hand, ask `kaku` to print it:

```sh
kaku mcp install --client claude-code --print --read-only
```

`--client` is `claude-code` (one `claude mcp add` line), `claude-desktop`, `cursor` or `generic` (an `mcpServers` block). It names this very file by its full path, carries `--read-only`, `--allow`, `--leased` and `--data-dir` as you give them, and contains no token: the server finds the service's token itself when it starts. The path and the SHA-256 of the file go to stderr, so what you paste stays clean and you can check what the client will run. It only prints. It never edits a client's configuration, because that file runs programs with your privileges. Not checked: the Claude Code line on Windows (it is refused; use `--client generic`), and Cursor's global configuration path (its documentation names only a project's `.cursor/mcp.json`).

`kaku` must be a path the client can run: use the full path of the binary you built (for example `/path/to/kakurega/target/debug/kaku`) or `/opt/kakurega/kaku` from the Linux package. Pass `--data-dir DIR` if the service was started with another data directory.

## What an agent can do

| Tool | Reads | Changes |
|---|---|---|
| `overview` | counts: setup problems, reviews by state, memories by privacy level, signed log head | nothing |
| `list_reviews`, `review_status` | state and counts only | nothing |
| `review_events` | what happened to the reviews after a position you remember: review id, stage name and time, never a detail | nothing |
| `start_review` | a folder path the owner allowed | starts a review; releases nothing |
| `request_approval` | | asks the owner to approve; the owner decides in a window or terminal |
| `verify_capsule`, `verify_log` | a capsule or saved log file, checked offline | nothing |

Every tool has an input schema that forbids extra fields, an output schema, and annotations. The published list is a contract: `crates/kk-cli/tests/mcp_tools.golden.json` fails the build when it changes, so a schema change is always deliberate.

## Resources and a prompt

Some clients show resources and prompts to their user instead of letting the model choose a tool. Both follow exactly the same limits as the tools they stand for (`--read-only`, `--allow`, `--leased`), and every read is recorded as the tool that produced it.

- `kakurega://overview` is the same counts and statuses as the `overview` tool.
- `kakurega://review/{id}/status` is the same as `review_status` for one review, for example `kakurega://review/job-3/status`. Any other address is "resource not found" and reaches nothing.
- `private_review` is a prompt with two arguments, `repo` and `recipient`. It lays out the steps (start the review, wait, ask for approval, tell the owner) and says the owner approves, never the agent. It is offered only while all three tools it uses are allowed, so it never walks an agent into a refusal. The two values are inserted as quoted text and must be plain text of reasonable length.

## A tool list you can pin, and report links

`kaku mcp --print-catalog-digest` prints the SHA-256 of the full tool list without serving anything. The same value is in the `initialize` answer (`_meta`, key `com.kakurega/catalogDigest`) and in every `tools/list` answer, next to `com.kakurega/listedDigest`, the digest of exactly the tools that connection was shown. A client that pinned the first value sees any change to a name, a description or a schema, including a swapped program. It proves the list did not change; it does not prove who built the program.

`verify_capsule` and `verify_log` hand over their full report as a `resource_link` (`kakurega://verification/ID`) beside the text, and never embed it. The link is read with `resources/read` on the same connection, which applies the same limits as the tool. The newest eight reports of a connection are kept; older ones read as "resource not found".

## Limiting what an agent can do

Three switches on `kaku mcp` narrow the tools an agent is offered. They combine, and none of them can widen what another took away.

- `--read-only` hides `start_review` and `request_approval`: the agent can only read counts and statuses. The same as setting `KAKU_MCP_READ_ONLY=1`.
- `--allow TOOL,TOOL` offers exactly the tools named and no others. A name that is not a tool is refused when the server starts.
- `--leased` keeps the two tools that act (`start_review`, `request_approval`) behind your permission. They are offered, and work, only while you hold a lease for them: a number of uses for a number of minutes, which you can take back at once. Without a lease a call changes nothing and the answer says how you can allow it.

You give and take leases with `kaku agent grant start_review --uses 3 --minutes 60`, `kaku agent revoke [TOOL]` and `kaku agent status`, or on the Agent access screen of the web workspace. The count is kept by the service, so restarting the agent's server does not reset it. A lease never approves or sends anything: approval stays yours alone.

## What an agent cannot do

An MCP client is a sink, not a principal with your privileges. The server has no tool to approve or deliver a release, and none that returns source files, findings, memory text, chat text or release contents. Files given to `verify_*` must end in `.kkg` or `.json`, be regular files under 20 MB, and every failure reads "cannot read that file", so the tool cannot be used to probe your file system.

## Checked

`cargo test -p kk-cli --test mcp_frames` and `--test mcp` (protocol, the golden contract, argument validation, uniform file errors) and `python3 tests/e2e/mcp_live.py` (a live agent session against a real sandboxed review that scans every response for source, finding and memory text).

## Limits

Counts and statuses are the only review data an agent receives. Anything richer needs an explicit release through the approval path, which an MCP client cannot trigger. There is no HTTP transport and no authentication beyond local access to the data directory.

## Reading files
See the notes in the engineering record: `verify_*` read only inside the agent's folder or the export folder with one uniform error; unknown tools are protocol errors; oversized lines are refused unbuffered; service calls time out after 30 seconds. `overview` reports `approvals_require_signature`; without an enrolled approver key the local admin token alone can approve.

## What is recorded

Every tool call an agent makes is written to the owner's signed event log as `mcp.call`: the tool's name and how it ended (`ok`, `error` or `refused`). The arguments and the answers are never recorded, and a tool name the agent made up is recorded as `unknown`. At most 120 calls a minute are written one by one; more are counted in an `mcp.calls_limited` entry, so an agent cannot fill the log. If the service cannot take the record, the call still answers. The owner sees these entries under Activity.


## Call limits

One connection may make 60 tool calls in any rolling minute and start 4 reviews in any rolling hour. A call over the limit is not run: the agent gets a tool error saying to try again in a number of seconds, and the refusal costs nothing against the limit. Lower either with `--calls-per-minute N` or `--starts-per-hour N` (1 to 1000). The limits count one connection's calls only; the owner's own use of the web page or terminal is not counted.

## Following a review without asking again and again

`review_events` takes an optional `wait_seconds` (0 to 25). With it, the call keeps looking and answers as soon as there is a new event, or after that many seconds with an empty list; without it, it answers at once. An agent can therefore loop on `review_events` with `after` set to the last `next` and `wait_seconds` of 20 and be told about each step as it happens, without a burst of calls. The wait is bounded at 25 seconds because clients commonly give up on a tool call after about thirty. A waiting call counts as one call against the limits above.

## Progress while `review_events` waits (ledger D-D09)
- Standard read (MCP specification 2025-06-18, Progress utility, fetched and read): a request may carry `params._meta.progressToken` (a string or an integer); the receiver may then send `notifications/progress` with that token, a `progress` number that must increase with each notification, an optional `total`, and an optional human-readable `message`; notifications must stop once the request has completed, must only reference tokens of requests still running, and should be rate limited.
- Built: a `review_events` call that carries a valid token and is waiting (`wait_seconds`) with nothing to report sends one progress notification about every five seconds: `progress` is the whole seconds waited, `total` the seconds it was told it may wait, and `message` the fixed words "Waiting for review events: N of M seconds" (no review id, folder or recipient). A token that is not a string of 1 to 128 characters or an integer is ignored and the call runs as before. Progress is sent only while the call is running and not cancelled, from the same thread that later sends the answer, so none can follow the answer or a cancellation. At most five notifications can come from one call (the wait is at most 25 seconds). Other tools answer at once and send none.
- Evidence: kk-cli mc93 (progress at about five and ten seconds with a string token, one with an integer token echoed as an integer, `progress` strictly growing, `total` right, fixed words only, no frame for seven kinds of unusable token, none for a call that does not wait, nothing left over after the answers) and mc94 (after a cancel, no more progress and no answer); 10 mutants, all caught.
- Not built: progress for tools that run long inside the service (a review's own stages come through `review_events` and the resource `kakurega://review/{id}/status`). D-D09 is BUILT for the one tool that waits.
