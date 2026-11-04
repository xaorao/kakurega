# Web workspace shell and companion API (task M1-02b)

## Target experience
A person opens a browser tab and works in a full workspace: projects, a task conversation with visible steps, a source viewer, an always-visible "where did my data go" panel, evidence review, release approval and capsule delivery. It must feel at least as fast and polished as OpenCode's and ChatGPT's web apps, while the page itself is never trusted with approvals or keys.

## References
- OpenCode (MIT, commit fe04656): `packages/app` (222k lines, SolidJS, Kobalte, Effect, Vite), `packages/ui` (58k lines, accessible component kit, i18n, strict typography rules), `packages/session-ui` (41k lines: message, timeline, tool rendering, diffs), `packages/server` (12.6k lines) and `packages/protocol` (5.4k). Read for this decision: package guidance for `ui`, `session-ui` and `app`; `server/src/auth.ts`, `cors.ts`, `pairing.ts`, `location.ts`; the dependency lists and directory structure of the four UI packages.
- Hermes `ui-tui` (React and Ink) and Codex `tui` (Rust) were only surveyed for the terminal decision (Milestone III), not for this one.
- Not read yet: `session-ui` message and timeline source, `protocol/src/api.ts`, `server/src/handlers`, the app's provider and session stores. They must be read before copying any code from them.

## Decision
- Stack: SolidJS with Vite and Kobalte primitives, on pnpm and Node. This matches OpenCode's UI packages (so timeline, tool and diff components can be adapted with credit under MIT) and OpenTUI (also Solid), so the terminal workspace in Milestone III can share the typed client and stores with the web. A fork of the whole `app` is rejected: it carries a coding-agent product, an Effect runtime and its own server. `ui` and `session-ui` pieces are adopted selectively after reading them, and restyled with Kakurega tokens (the "blank." direction, replaceable).
- The browser talks to the Rust companion over a typed HTTP and event-stream API defined by us. Nothing in OpenCode's server is reused.

## What OpenCode's local server does that we will not
- CORS accepts any `http://localhost:<port>` and `http://127.0.0.1:<port>` origin and any request without an `Origin` header. Another local web app, or a dev server on a different port, is therefore trusted (the WS01 attack).
- The password is optional and defaults to none; sessions are stateless 30-day HMAC tokens that cannot be revoked except by changing the password.

## What we take
- One-time pairing codes (16 random bytes, base64url, 5-minute lifetime, consumed on first use).
- Cookie names that include the port so two local servers do not collide.
- Same-origin check on cookie-bearing requests (`Origin` host must equal the request `Host`).
- The rule that browser code on non-secure origins cannot use `crypto.randomUUID`, `crypto.subtle`, clipboard or service workers: the companion is served only from a secure context (loopback HTTPS or localhost), and passkeys need that anyway.

## Companion API rules (tests WS01, WS02, WS05-WS07, new WS11-WS14)
Bind to loopback only; reject any `Host` that is not the exact companion host (DNS rebinding); `Origin` must equal the exact companion origin for browser requests, and a request with no `Origin` is accepted only with a native-client credential, never from a cookie; sessions are stored server side with a short lifetime and instant revocation; pairing codes are single use; every state-changing request carries a CSRF token bound to the session; approvals are never accepted from the browser session alone (passkey plus independent exact-bytes view).

## Not verified
Performance of Solid with long streaming transcripts at our scale; accessibility of Kobalte-based components under our themes; whether `@pierre/diffs` and `@pierre/highlights` are permissively licensed (to be checked before use).
