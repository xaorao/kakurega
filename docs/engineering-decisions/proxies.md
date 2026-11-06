# Proxies named in the environment (ledger C-18)

## Finding
Found by probing, not by reading: with `HTTP_PROXY` set, the client library (ureq 3) sends a call to the local model through the proxy (`CONNECT 127.0.0.1:9` reached a listener acting as the proxy). `ureq` reads the proxy variables by default (`src/config.rs` line 1073 `proxy: Proxy::try_from_env()`, read by me). For a route that is called "device only" this is a privacy defect: the connection to the model, and the prompt in a plain-HTTP call, would travel to whatever the variable names. The same default applied to the fetcher (whose address checks and pinned connection a proxy would step round), the terminal's session to the local service, and the `kaku approve` admin session (the admin token).

## Decision per client
- Local model route (`kk-route`, one shared builder), the model probe in the runner, the terminal-to-service client, the admin client, and the page fetcher (both its calls): no proxy, whatever the environment says. A fetch that must go through a mandatory corporate proxy is therefore not possible; that is a deliberate loss, because the fetcher's guarantee (it connects only to addresses it checked) cannot hold through a proxy.
- Telegram bridge (talks to a public service, no pinning): keeps the library default and so honours `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY`. NOT tested end to end here (no Telegram bot access); the default itself is VERIFIED by reading the library source.

## Evidence
kk-route px01 (a listener standing in for the proxy is never contacted by `complete` or `stream`; the model's own address is), kk-fetch px02, kk-tui px03, kk-cli px04; removing `.proxy(None)` from each of route, fetch, tui and admin makes its test fail.

## Not covered
The runner's model probe (`/api/tags`) was changed the same way but has no test of its own; other tools the sandboxed worker may run do their own networking (the worker has no network).
