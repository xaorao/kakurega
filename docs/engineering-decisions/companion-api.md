# Local companion (web local-companion mode; tests WS01-WS14)

## Target experience
Open one local address, paste or follow a one-time pairing link, and use the workspace. No other web page, local program or network neighbour can use the companion, and the page can never approve a release by itself.

## References
OpenCode `packages/server` (MIT, commit fe04656): pairing codes, cookie naming by port, same-origin rule (`web-shell.md` lists what it does and what we do not copy); surface matrix `SURFACE_PARITY_AND_TRUST_BOUNDARY_MATRIX.md` section 3.2.

## Approach (`kk-companion`)
axum on `127.0.0.1` with an operating-system chosen port. One guard in front of every route: the `Host` header must equal the exact loopback address (DNS rebinding), `Sec-Fetch-Site` must be same-origin or none, a request that is not GET or HEAD must carry an `Origin` equal to this page, and any `Origin` that differs is refused even if it is another localhost port. API routes need a server-side session found by a hashed random cookie (`HttpOnly`, `SameSite=Strict`, named for the port, 8 hours); state-changing requests need that session's CSRF token in a header. Pairing codes are 16 random bytes, 5 minutes, single use, stored hashed, with a lockout after five wrong guesses; the right code also waits during a lockout. The admin interface (for the local `kaku` tool) takes only a bearer token, and any request that carries an `Origin` or a cookie is refused, so a web page can never reach it. Every response, errors included, carries a strict content security policy (no inline script or style, no framing), `nosniff`, no referrer, no store, and cross-origin isolation headers. Static files are served from one directory with no dot files, no traversal, no symlinks; unknown paths without an extension fall back to the app shell. Bodies are limited to 1 MiB.

## Results (VERIFIED_BY_RUN, `cargo test -p kk-companion`)
9 tests over a real socket with raw requests (so the Host header can be forged): foreign hosts (6 forms), foreign origins (5 forms) plus missing Origin plus cross-site fetch plus another localhost port, single-use / expiring / locked-out pairing, server-side session revocation and per-session CSRF, cookie attributes, security headers on success and error responses, admin access rules, static file escapes (9 traversal and link forms), oversized body. Mutation check: 11 of 11 applicable mutations caught.

## Not covered
Job and approval endpoints (next); passkey approvals (Milestone II; for now a local terminal approval); TLS (loopback http, secure context only by the browser's localhost rule); a real browser driving the UI (Playwright, next); Windows and Linux desktop browsers; behaviour behind proxies; rate limiting beyond pairing.
