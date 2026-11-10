# MCP client behind the broker (ledger G-GH-64, G-GH-65, G-GH-67, D-D22; status: design, nothing built)

## Target experience
1. The complete experience: the owner adds an MCP server (a program they trust enough to run) in Settings or the terminal, sees the tools it offers, and chooses per tool: off, ask each time, or allowed. A coding or review task can then call an allowed tool; every call goes through the broker like any other tool, its result is treated as untrusted text, and the owner can see what was called with what.
2. Strongest references: rmcp 3.8.0 (Apache-2.0, d66a74d; already a dependency of kk-cli) for the protocol, goose (Apache-2.0, 71dd9a9) extension manager for the lifecycle, IronClaw (Apache-2.0) MCP lane for the mediation, Ghast's public MCP docs (closed, study only: per-turn tool switches).
3. Approach: the protocol is the official SDK, not hand-written. Servers run on the owner's computer as child processes started by the service, never inside the review sandbox and never by a model. A tool becomes visible to an agent only when the owner set it to allowed; "ask each time" raises a kernel approval. The broker, not the worker, makes the call.
4. Quality criteria: a server that hangs, floods, crashes or lies cannot hang the service, fill memory or widen its own permissions; a tool's description and result can never act as an instruction; nothing is called that the owner did not allow; secrets reach a server only by name from the vault, never through a model.

## Reading depth (honest)
- rmcp d66a74d: `transport/async_rw.rs` 240-340 and `service.rs` 1370-1780 read earlier (the serve loop, cancellation by id). `service/client.rs` (2942 lines), `transport/child_process.rs` (418), `handler/client.rs` (464): NOT READ yet. kk-cli's existing use of the client in tests: read earlier.
- goose and IronClaw MCP lanes: NOT READ for this component.

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
