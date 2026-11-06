# Agent leases: the owner decides what a connected agent may start

## Target experience
- An AI agent connected over MCP can always read. Starting a review or asking for an approval is something the owner allows for a limited number of uses and a limited time, and can take back at once.
- The owner sees it in the web app ("Agent access") and the terminal (`kaku agent grant|revoke|status`). The agent that is refused is told what happened and what the owner can do.

## Sources (read by me)
- IronClaw b0b999d (MIT OR Apache-2.0, https://github.com/nearai/ironclaw), `crates/kernel/ironclaw_authorization/src/lib.rs`: the lease store and its status machine (Active, Claimed, Consumed, Revoked), `CapabilityGrant` with `expires_at` and `max_invocations` (`crates/contracts/ironclaw_host_api/src/capability.rs` 370-392), claim and consume under a lock with compare-and-swap so a limited lease cannot be spent twice at once, revoke idempotent, wrong-scope lookups behave as unknown. READ: lib.rs 150-330, 690-800, 1170-1190, 1360-1500; NOT READ: the rest of the 1680 lines, the 1434-line lease contract tests.
- OpenCode fe04656 (MIT), `packages/core/src/permission.ts` (388, FULL) and `permission/saved.ts`: rules where a deny anywhere beats everything, "always" saves an allow rule and settles other waiting requests it now covers, a reject settles every waiting request of the session, unattended work gets a fixed rule set and anything else is denied with a reason, closing denies everything pending, a missing agent profile denies all.
- Ghast MCP docs (public docs of a closed product): per-turn tool switches, no tiers or limits. Ideas only.

## Decisions
- ADAPT the idea of limited, expiring, revocable grants with atomic spending. No code copied; written here on our SQLite store.
- BUILD: spending happens in the service (one UPDATE under the store lock), so restarting the MCP process cannot reset a count. IronClaw consumes after dispatch; we reserve first and give the use back if the call fails, so two calls at once cannot both take the last use and a failed call costs nothing.
- BUILD: opt-in `kaku mcp --leased`. Plain `kaku mcp` is unchanged, because existing connections must not break. `--read-only` and `--allow` stay ceilings a lease can never widen.
- EXCLUDE: OpenCode's "always allow" saved rules and auto-approve. Here the agent can only ask; the owner's approval of a release is never delegated.
- Fail closed: if the service cannot be asked, the tools that act are hidden and refused.

## Behaviour
- Two tools can be leased: `start_review`, `request_approval`. A lease has uses (1 to 1000) and minutes (1 to 1440); a new grant for a tool replaces the old one.
- Listing: in leased mode a tool that acts is listed only while a lease with uses left exists. Every `tools/list` asks again, so a grant or revoke shows at the next listing; a client with an older list still gets a clear refusal or success at call time.
- Call: arguments are checked first (a malformed call asks and spends nothing); then one use is taken; then the work runs; an error result gives the use back.
- Owner: `GET/POST /api/agent-access[...]` (paired browser) and `/admin/agent-access[...]` (admin token). Only `/admin/.../use` and `/refund` are for the MCP door; the page cannot spend a lease.
- Log: `agent.lease.granted|used|refused|refunded|revoked`, with tool names and counts only, never paths or review data.

## Known limits (written next to the claim)
- Anyone who can already run programs as the same operating-system user can read the admin token and call every door. A lease protects against an agent that misbehaves through MCP, not against a computer that is already taken over. The web screen says so.
- A use spent by a call that dies with the process (killed mid-call) is not given back.
- The tool list is not pushed to the client when a lease changes (`listChanged` is false); clients that cache the list learn at their next listing or call.
- Not built: terminal screen (TUI) for leases, per-tool different limits per time window, leases for the read tools, an owner-signed lease file. `kaku agent` covers the terminal.

## Evidence
- Service: sv63-sv67 (use, exhaust, expiry via a test clock, revoke, replace, validation, owner-only, restart keeps counts, log without paths, refund bounded); 16 mutants caught.
- MCP door: mc22-mc29 against a service stand-in; ag01, ag02, ml01 unit; 12 mutants caught (three first survived and led to mc24's listing check, mc26's message check and mc29).
- Real service and real sandboxed review: `tests/e2e/mcp_live.py` leased block (11 checks, 43/43). Browser: Agent access screen checks in `tests/e2e/run.mjs` (200/200), axe on the screen.
