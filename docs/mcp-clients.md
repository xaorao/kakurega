# MCP clients: what each one expects

What the clients we have read do at connection time, and how `kaku mcp` meets it. The source for each row is the client's own repository, read for the capability matrix (`docs/engineering-decisions/capability-matrix/D-*.md`, row D37). None of the opening exchanges in `crates/kk-cli/tests/mcp_clients.rs` was captured from a running client: each is built from what that client's code sends, so a client that changes its handshake is not detected until it is replayed for real.

| Client | What it relies on | What `kaku mcp` does |
|---|---|---|
| opencode | server `instructions` go into the model's prompt; structured output preferred; permission is asserted per `server_tool` action name | short fixed `instructions` (counts and statuses only); every tool has an `outputSchema`; tool names are plain |
| Cline | declares empty capabilities; rewrites tool names longer than 64 characters; caches oversized results to disk | names are at most 16 characters; results are small counts |
| goose | trusts `readOnlyHint`; reloads its tool list on `tools/list_changed` | every tool states `readOnlyHint`; `listChanged` is `false` and never sent |
| Codex | treats model-supplied arguments as non-authority; auto-accepts empty confirmation prompts when approvals are off | no tool approves or delivers; an unknown or oversized tool name is an error result |
| Hermes | copies tool schemas from one definition; turns exceptions into JSON text | the tool list is generated from one catalogue, pinned by `mcp_tools.golden.json` and the catalogue digest |
| Claude Code | offers `roots` and `elicitation` | neither is used; the server asks the client for nothing |

Protocol versions answered in kind: 2025-06-18, 2025-03-26, 2024-11-05. A version we do not know is answered with the newest one we speak, as the specification says.

Tested by `cargo test -p kk-cli --test mcp_clients`: every row's opening succeeds, `ping` answers, every tool has an object `inputSchema`, an `outputSchema`, a `readOnlyHint`, a name of at most 64 plain characters and a description; a call with no service behind it is an error result rather than a protocol error; no approving tool exists.

Errors: when the service refuses a request, an agent sees the error code and, only for a short list of codes whose text is a fixed sentence about the request itself (`BAD_JOB`, `NO_JOB`, `NO_APPROVAL`, `TOO_MANY`, `NOT_APPROVED`, `NOT_QUEUED`, `NOT_DELIVERED`, `BAD_KEY`, `DENIED`), that sentence, and only if it is under 120 characters, one line and free of slashes, backslashes, `@` and backticks. Every other message is replaced by its code and a note that the owner sees the details. Tested by `fe01` in `crates/kk-cli/src/mcp.rs` (8 mutants caught).
