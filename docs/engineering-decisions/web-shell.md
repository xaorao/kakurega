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

## Search across kept conversations, and renaming one (ledger B-W07, G-GH-23)
- Reference: OpenCode `search.ts` mechanics as recorded in the matrix (multi-source, stale answers dropped, newest first); not re-read for this row, so NOT READ beyond the matrix. LibreChat's search is a Meilisearch service (not read) and would put conversation text in a second store, which this product refuses; ClaraVerse searches names only.
- Behaviour: `GET /api/sessions/search?q=` reads only conversations the owner chose to keep, inside the encrypted store. Every word of the question (up to 6, 64 characters each, 200 in all, plain text, no control characters) must appear, in any case, in the same conversation's title or turns. Results: up to 20, newest first, with up to two snippets of about 140 characters. A `%`, `_` or quote is just a character: nothing is a pattern and nothing reaches SQL. Withheld files are never in a conversation, so they cannot be found. `POST /api/sessions/{id}/rename` applies the same rules as a new title (1 to 80 characters, no control, direction or tag characters, no secrets) and the log records the length, never the title.
- The page ignores an answer to an older question when a newer one was asked (a sequence number, 200 ms pause); the terminal has `/find WORDS` and `/rename ID TITLE`.
- Evidence: kk-service sv89 (content, title, every-word rule, snippets, 6 hostile queries, 5 refused titles, 404, log, forgotten conversations disappear), kk-tui tu43, browser e2e 253/253 (a said word is found with its context, a word from another conversation finds nothing, rename shows in the list and in search). 15 mutants caught after two assertions were added.
- Not covered: an in-flight request is not aborted (older answers are dropped, not cancelled; the "one in flight" check of the matrix is not built); ranking beyond newest-first; archive; more than 20,000 turns (older turns are not searched); fork; Hindi read by a person.

## Keyboard shortcut list (ledger B-W10)
- References (matrix, listings only; NOT READ in source): LibreChat `shortcutDefinitions` with mac and other display strings and a recorder, AnythingLLM `SHORTCUTS` and help modal, OpenCode keybind listing. Taken: one declarative table, mac and other spellings, a help dialog opened by `?`.
- Behaviour: `apps/web/src/shortcuts.ts` is the one table (id, scope, both spellings, label, whether it works while typing, the test that says a key press is it). The page-wide shortcuts (command palette, search the reviews, the list itself) are found through the table (`appShortcut`), so what the dialog lists is what the page answers to; `?` opens the list only when the cursor is not in a text box, Ctrl or Command and `/` opens it anywhere. The chat entries are tied by tests to the functions that really handle those keys (`stopsReply`, `historyKey`, `opensHistoryPicker`).
- Found while building it: my first table said Ctrl+G opens the owner's editor in the chat box. In the web workspace Ctrl+G stops a reply (the editor hand-off is the terminal's). The tie tests exist because of that.
- Evidence: web unit kb01–kb06 (92 pass in the web suite), 5 mutants caught, browser e2e 259/259 (nine rows, one per shortcut; `?` typed in the message box is a question mark; Ctrl+/ opens it while typing and the cursor returns; axe).
- Not covered: rebinding and a key recorder (not built; the matrix said "later"); the terminal's own keys (not in this table); Enter-to-send is in the table but its handler is in `Chat.tsx`, not tied by a test.

## Usage ledger per route (ledger G-GH-16)
- Reference: Ghast's usage page (changelog 0.1.27 and 0.1.33: per-model cost, custom date range, response speed; no source, NOT READ). Decision: BUILD without cost. This program knows no prices (local models cost nothing; a remote route's price is the owner's contract), so inventing a cost would be a guess; the page says so.
- Behaviour: every finished chat reply adds one row to `usage_ledger` in the encrypted store: when, route, class, whether it is the test model, tokens read (the model's own count when it gave one, else the estimate, and which), tokens written (estimate), milliseconds from the question arriving to the reply finishing. No words, no conversation id. 400 days are kept. `GET /api/usage?from=&to=` (seconds; end not included; default the last 30 days; at most 366 days; unknown fields refused) answers per route and in total, with average and slowest time and tokens a second. Activity screen card with a day range; terminal `/usage [DAYS]`.
- Evidence: kk-service sv90 (three replies: the ledger adds up to what each reply reported, measured versus estimated, range in and out, 6 refused queries, no words; a measured runner is counted as measured), kk-tui tu44, browser e2e 265/265 (route row, speed, total, no conversation word, empty range, reversed range, over a year, axe). 9 mutants caught; the "default end is one second ahead" one is timing-dependent and was not run.
- Not covered: document questions (`ask`) and reviews are not in the ledger (only chat replies); streamed replies' time includes the whole stream; no prices and so no cost; "ledger sums equal provider invoice" cannot be tested without a provider.
