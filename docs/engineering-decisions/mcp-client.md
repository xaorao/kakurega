# MCP client behind the broker (ledger G-GH-64, G-GH-65, G-GH-67, D-D22; status: the client crate is built and tested; wiring to the service, broker, web and terminal is not)

## Target experience
1. The complete experience: the owner adds an MCP server (a program they trust enough to run) in Settings or the terminal, sees the tools it offers, and chooses per tool: off, ask each time, or allowed. A coding or review task can then call an allowed tool; every call goes through the broker like any other tool, its result is treated as untrusted text, and the owner can see what was called with what.
2. Strongest references: rmcp 3.8.0 (Apache-2.0, d66a74d; already a dependency of kk-cli) for the protocol, goose (Apache-2.0, 71dd9a9) extension manager for the lifecycle, IronClaw (Apache-2.0) MCP lane for the mediation, Ghast's public MCP docs (closed, study only: per-turn tool switches).
3. Approach: the protocol is the official SDK, not hand-written. Servers run on the owner's computer as child processes started by the service, never inside the review sandbox and never by a model. A tool becomes visible to an agent only when the owner set it to allowed; "ask each time" raises a kernel approval. The broker, not the worker, makes the call.
4. Quality criteria: a server that hangs, floods, crashes or lies cannot hang the service, fill memory or widen its own permissions; a tool's description and result can never act as an instruction; nothing is called that the owner did not allow; secrets reach a server only by name from the vault, never through a model.

## Reading depth (honest)
- rmcp d66a74d: `transport/async_rw.rs` 240-340 and `service.rs` 1370-1780 read earlier (the serve loop, cancellation by id). `transport/child_process.rs` lines 1-260 of 418 (FULL for the spawn, the builder, graceful shutdown and kill-on-drop; the rest is tests and a `which` helper); `service/client.rs` (2942 lines): the function map only (NOT READ in depth); `handler/client.rs` (464): NOT READ yet. kk-cli's existing use of the client in tests: read earlier.
- goose and IronClaw MCP lanes: NOT READ for this component.

## What the transport reading showed (VERIFIED in the code)
| Finding | Decision |
|---|---|
| The child inherits the whole environment of the parent (the builder only sets stdio) | IMPROVE: the service builds the command with `env_clear()` and passes only the owner's named variables and a fixed safe PATH |
| stderr is inherited by default, so a server writes into our terminal | IMPROVE: stderr is piped, read with a size cap and kept for the owner's "why did it fail" view, never shown raw to a model |
| CORRECTION (read `transport/async_rw.rs` 50-135): one incoming line IS capped (default 16 MiB, `with_max_line_length`; an oversized line is dropped and the connection stays open), but total output over many lines is not | IMPROVE: spawn the child ourselves and build `AsyncRwTransport` with a small line cap; cap each result's bytes, the number of tools listed and the time per call (proven against a flooding fake server) |
| Shutdown is polite then SIGTERM then kill, 3 s each; the child is killed on drop through a spawned task (needs a running runtime) | REUSE the sequence; our service shuts servers down explicitly and tests that no process is left |
| `which_command` resolves a name through PATH | EXCLUDE: the owner gives an absolute path to the program; a bare name is refused so PATH cannot choose a program |

## What the client call path showed (VERIFIED, `service/client.rs` 2134-2209)
- `call_tool` drives "input_required" rounds by itself through the local handler (a server can ask the client for more input and the client answers, up to a round cap), and `call_tool_once` does not. Decision: use `call_tool_once` only and treat anything but a complete result (an input request or a task) as an error, so a server can never pull the model or the owner into its own loop.

## Threat model (what the design must hold against)
| Threat | Required property |
|---|---|
| A server's tool description or result contains instructions | descriptions are shown to the owner and to the model as data inside a fixed frame; results are untrusted and labelled; neither can change policy |
| A server renames or adds a tool after approval (rug pull) | the owner's choice is pinned to the tool's name and a hash of its description and input schema; a change makes the tool wait for the owner again |
| A server floods output or never answers | per-call timeout, result byte cap, a cap on tools listed, a cap on calls per task |
| A server is started with the service's secrets | a minimal environment; only the variables the owner named, filled by reference from the vault |
| A server runs somewhere it can write the project | OUT OF SCOPE for the sandbox (the server is the owner's own program on the host); said plainly in the UI: "this program runs on your computer with your permissions" |
| A model calls a tool the owner did not allow, or by another name | the broker looks the name up in the allowed list at call time; unknown means refused |
| A tool that writes or sends things is called without the owner knowing | tools default to off; "allowed" needs the owner to mark a tool read-only or accept that it can act; non-read-only tools raise an approval per call unless the owner granted a lease |

## Decisions (before reading, to be confirmed by the reading)
| Capability | Decision |
|---|---|
| Protocol | REUSE rmcp client with the child-process transport, no new protocol code |
| Transports | stdio only. Streamable HTTP and SSE: EXCLUDE for now (a network listener and auth surface; revisit with hosted mode) |
| Registry of servers | owner-added only; no remote registry, no one-click install from a URL |
| Tool choice | per tool, pinned to a hash; default off |
| Prompts, resources, sampling, roots, elicitation from the server | EXCLUDE: the client offers none of them, so a server cannot ask the model anything |
| Tool-call approval | reuse the kernel lease and approval path already used by agent leases |

## Owed before building
1. Read rmcp `service/client.rs`, `transport/child_process.rs`, `handler/client.rs`; goose's extension manager lifecycle (start, health, restart, shutdown); IronClaw's MCP lane (how the call is mediated).
2. Write the capability matrix and the behaviour spec; write the hostile-server fixtures first (a fake server that hangs, floods, changes a tool, returns an injection).

## Built (kk-mcpc, commit b5f42d5)
- `kk-mcpc` (layer 2): `Server::start/list_tools/call/stop`, `ServerConfig`, `Limits`, `ToolInfo` with a pin, `Outcome`. Uses rmcp 3.8.0's `AsyncRwTransport` over a child we spawn ourselves: `env_clear()` plus a fixed PATH plus the owner's named variables, stderr piped and kept to the last 8 KiB, line cap, per-call timeout (a timeout or a protocol failure marks the server dead and kills it), result cut and cleaned, non-text parts counted and left out, only tools from the last listing can be called, arguments capped, `call_tool_once` only (a server's request for more input is an error and stops it).
- Evidence: a scripted fake server with 12 hostile modes (hang, crash, flood, oversized line, too many tools, changed tool, injection text, non-text parts, tool error, environment dump, noisy stderr); 14 tests mc01 to mc14; 13 mutants, all caught by test failures against a green baseline (one survived until a relative path that exists was added to mc02).
- Not covered: a server that sends endless pages of its tool list within the time limit (memory is bounded only by the call timeout); a server that writes many small lines within the time limit (no total-bytes cap); the server's own children are not killed with it (no process group); a request for more input is untested because the fake server cannot produce one (the code path stops the server).

## Built (service wiring)
- `crates/kk-service/src/service/mcp.rs`: owner-only endpoints `/api/mcp` (list), `/api/mcp/servers` (add, with the program and environment checked), `/api/mcp/servers/remove`, `/api/mcp/discover`, `/api/mcp/tools` (off or allow, needs the pin the owner read, else 409), `/api/mcp/test` (the owner's own call). A server runs only for one listing or one call and is stopped again. Environment values live in the encrypted store and are never returned. A tool whose pin changed between allowing and calling is not called and goes back to off; a new listing does the same and names it. Changing a server's program, arguments or environment clears its tool choices.
- Evidence: sv200 to sv205 (7 tests incl. a real fake server process); 8 service mutants, all caught against a green baseline (a first run was invalid: one mutant hung and two survived; tests sv204 and sv205 were added).
- Mode `ask` (approval per call) is NOT built; only off and allow exist, and the API refuses `ask`.

## Not built yet (owed)
1. DONE above: the server list and the per-tool choice.
2. DONE for coding mode: `kk_review::code::run_task_with` offers the tools the owner allowed (`mcp__SERVER__TOOL`, schema stored with the pin and capped at 4 KiB), calls them through the service (which re-checks mode and pin at call time), frames each answer as untrusted text, cuts it at 8000 characters, and allows 8 calls per task; a call is a signed log line (`mcp.called`, size and caller, never the text). Tests lp14 to lp19 (6 mutants plus 2 more caught), sv206 (probe runner), browser test `tests/e2e/mcp_web.mjs` (10 of 10 in a real browser). NOT built: the review sandbox gets no MCP tools (a model in the sandbox must not reach host programs), the per-call approval (`ask`), a real model calling a real MCP tool end to end (NOT VERIFIED), the terminal commands.
3. Web and terminal screens; the secret-by-name filling of variables from the vault.
4. The goose and IronClaw lifecycle reading, and the process-group kill.

## Real-model run (OBSERVED, this Mac)
- qwen2.5-coder:7b through the real service (`kaku serve`, HTTP with a paired session): the owner added the scripted fake MCP server, checked it, allowed `echo` with its pin, and started a coding task telling the model to call `mcp__docs__echo` with "ping". The model made the call, the service started the server, called it, stopped it, and the model reported the answer `echo: ping` (VERIFIED by run). The same task also asked it to fix a bug and it did not (small-model limit; the summary said no change was made). A third-party MCP server (not our fake) was NOT tested. Terminal commands: tu_mc01 to tu_mc03, 7 mutants caught.

## Effect claim and the owner's confirmation (built after the first wiring)
- `kk_mcpc::Effect` (`read_only`, `may_change`, `unknown`) comes from the server's own `readOnlyHint` and is part of the pin, so a flip changes the pin. It is the server's CLAIM and is shown as such ("the server says"); it never grants anything. Allowing a tool that is not `read_only` needs `knowing: true` in the request (web: a second checkbox; terminal: `/mcp allow SERVER TOOL knowingly`), else 409 `MCP_MAY_CHANGE`. Turning a tool off never needs it. This is a confirmation, not the per-call approval (`ask`), which is still not built.
- Evidence: mc15, mc16 (4 mutants), sv207 (4 mutants), tu_mc01 to tu_mc03 (3 mutants), browser `tests/e2e/mcp_web.mjs` 14 of 14 including the disabled-until-confirmed checkbox.
