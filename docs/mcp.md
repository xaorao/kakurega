# Kakurega MCP server

`kaku mcp` speaks the Model Context Protocol (version 2025-06-18) over standard input and output, one JSON message per line. It lets an agent start and watch reviews on your computer without ever seeing what is in them.

## Use it

The service must be running (`kaku serve`, or the desktop app, or the terminal workspace). Then add the server to an MCP client, for example:

```json
{ "mcpServers": { "kakurega": { "command": "kaku", "args": ["mcp"] } } }
```

`kaku` must be a path the client can run: use the full path of the binary you built (for example `/path/to/kakurega/target/debug/kaku`) or `/opt/kakurega/kaku` from the Linux package. Pass `--data-dir DIR` if the service was started with another data directory.

## What an agent can do

| Tool | Reads | Changes |
|---|---|---|
| `overview` | counts: setup problems, reviews by state, memories by privacy level, signed log head | nothing |
| `list_reviews`, `review_status` | state and counts only | nothing |
| `start_review` | a folder path the owner allowed | starts a review; releases nothing |
| `request_approval` | | asks the owner to approve; the owner decides in a window or terminal |
| `verify_capsule`, `verify_log` | a capsule or saved log file, checked offline | nothing |

Every tool has an input schema that forbids extra fields, an output schema, and annotations. The published list is a contract: `crates/kk-cli/tests/mcp_tools.golden.json` fails the build when it changes, so a schema change is always deliberate.

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

