# Capability disposition ledger (RCR)

Every row of the seven capability matrices (`capability-matrix/`), with what happened to it. This is the evidence the retrospective gate asks for: each applicable row is BUILT (with a pointer to its evidence) or EXCLUDED (with the reason), or it is listed as owed with its value. Rows nobody has checked are UNCHECKED; the count is at the top and a row never moves out of UNCHECKED without evidence.

Statuses: BUILT (done, evidence given) | PARTIAL (part done, rest named) | EXCLUDED (decided against, reason) | KEPT (ours already better or the same; nothing to do) | LATER (belongs to a later surface or needs something external) | OWED-H / OWED-M / OWED-L (applicable, not built; value to the owner) | UNCHECKED.

Matrix A (terminal) is written as sections A01-A46 rather than tables and is listed separately at the end.

## Summary (computed from the tables below)

- Rows: 372. BUILT 137, PARTIAL 94, KEPT 24, EXCLUDED 40, LATER 77 (a later surface or something external), UNCHECKED 0.
- Owed and applicable: OWED-H 0, OWED-M 0, OWED-L 0. PARTIAL rows each name what is missing.
- Honesty note: the BUILT statuses were assigned by me from the code, tests and records named in each row; the first pass by other agents (the matrices) was never used as evidence that something exists. A few rows say NOT VERIFIED where I could not confirm a detail.

### High-value rows
1. B-W02 / G-GH-84: the page that says what stays on this computer and what can leave: BUILT (privacy-page.md).
2. D-D35: every MCP call in the signed log: BUILT.
No OWED-H row remains. The medium rows below come next, in order.

## Matrix B: Web workspace

| Key | Capability | Matrix decision | Status | Evidence or reason |
|---|---|---|---|---|
| B-W01 | First-run flow | BUILD (own flow). ADAPT the idea of a data-handling step. No tour library | PARTIAL | pairing then the readiness screen with one-click repair is the first run (e2e); no data-handling step (see W02) |
| B-W02 | Data-handling disclosure | IMPROVE. Do not ship a hand-written policy table; derive the panel from the kernel's route | BUILT | Where your data goes page, generated from live facts (privacy-page.md; sv77-sv81, pv01-pv03, e2e 231/231) |
| B-W03 | Consent sentence before an irreversible action | ADAPT the pattern as a component used on release approval and on any route downgrade; copy | BUILT | forget asks twice and says it is permanent; release approval shows exact files and needs the typed digest (e2e) |
| B-W04 | Pairing | EXCLUDE reuse; keep ours. QR/camera pairing for a phone is EXCLUDED until `hosted-mode.md` | KEPT | ours kept (pairing link and one-time code) |
| B-W05 | Connection loss and recovery | ADAPT the grace-period hook (MIT, tiny) and add a distinct "session expired, pair again" s | BUILT | live.ts reconnect with backoff, unreachable state, three-miss notice (lv01-lv06, e2e) |
| B-W05b | Workspaces / projects | BUILD, with AnythingLLM's "workspace owns documents + threads + memory scope + route prefe | PARTIAL | projects scope notes and chats (f808980); documents are per folder; no single workspace object |
| B-W06 | Conversation and job list | ADAPT unread-from-timestamps rule, pin, archive. Delete is a kernel operation (see W44) | PARTIAL | pin (max 10) and archive for kept conversations in the web list; no unread markers, no terminal (web-shell.md) |
| B-W07 | Search across conversations | ADAPT `search.ts` (MIT) for the multi-source/abort/rank mechanics. IMPROVE: full-text inde | BUILT | search across kept conversations (content and title, every word, bounded, owner session) on web and terminal; stale answers dropped, not cancelled (web-shell.md) |
| B-W08 | Find in the open conversation | ADAPT the technique (no DOM rewriting, so no CSP or XSS issue). Fall back to no highlight  | BUILT | in-conversation find: count, stepping, browser highlight, repaint on change; OpenCode search-controller.ts read FULL; Cmd+F takeover excluded by decision (web-shell.md) |
| B-W09 | Command palette | ADAPT `command.tsx` registry (MIT, Effect `Schema` used for persistence only: replace). IM | BUILT | command palette with ranking, group headers, page and section keys (pm01-pm10, e2e) |
| B-W10 | Keyboard shortcuts, help, rebinding | ADAPT: declarative table + help dialog (opened by `?`/Cmd+Shift+/), recorder later | PARTIAL | declarative shortcut table, help dialog generated from it (? and Ctrl+/), tied to the real handlers; no rebinding or recorder (web-shell.md) |
| B-W11 | Multiple open threads (tabs) | BUILD later, low priority; not needed until background agents run in parallel | LATER | tabs: not needed until background agents run in the page |
| B-W12 | Event stream transport | BUILD. Server-sent events from `kk-companion` through the same guard as every route (Host, | BUILT | server-sent change notices with ping and reconnect (sv37, lv01-lv06); Release tab uses them (row 17) |
| B-W13 | Streaming markdown | ADAPT patchmark (MIT) with notice. Required changes: (a) after sanitising, rewrite or drop | BUILT | safe markdown renderer with copy buttons (mk tests, e2e) |
| B-W14 | Code blocks and highlighting | ADAPT worker + tokenizer. NOT VERIFIED: whether `@pierre/highlights` needs WebAssembly (Sh | PARTIAL | own one-pass scanner for 7 language families, classes only, joins back to the exact text (fuzzed), size-limited; used for chat code blocks and reproducers; library/worker not read; no diffs, no streaming incremental (web-shell.md) |
| B-W15 | Message actions | ADAPT fork-before-prompt and sibling switcher as logic ported to Solid (MIT). IMPROVE: bra | PARTIAL | message actions in the web chat: copy, edit and branch, answer again, version switcher; branches are new kept conversations, history never rewritten; no terminal or side-by-side compare (web-shell.md) |
| B-W16 | Stop, cancel and leave-guard | BUILD: `POST /api/threads/:id/cancel` through the kernel; the guard only warns, it grants  | BUILT | Stop and Esc and Ctrl+G, draft returns, leave aborts and keeps the draft (cm04, e2e) |
| B-W17 | Reasoning and thinking display | ADAPT the three-mode switch. Reasoning text is untrusted model output: collapsed by defaul | LATER | local models here expose no separate reasoning text |
| B-W18 | Long transcript performance | ADAPT virtualiser and deferral after a profile on a synthetic 5,000-event run; do not adop | EXCLUDED | profiled: Memory 5,000 notes 0.36 s, 20,000 notes 1.3 s; chat send 60 ms at the 400-turn kept limit, 384 ms at 1,000; no virtualiser (it would break find, selection, readers); revisit triggers recorded; gate runs the measurement (web-shell.md) |
| B-W19 | Tool-call and step display | ADAPT `BasicTool` and the visualizer registry (both MIT). IMPROVE: each card is keyed by ` | LATER | tool-call display waits for the worker loop in the web |
| B-W20 | Turn structure and grouping | ADAPT the shape; do not import Effect (`Data`, `Match`, `Schema`); use plain discriminated | LATER | turn grouping waits for tool calls |
| B-W21 | Errors, retry and limit rows | ADAPT tool-error-card shape; error text must pass through the same redaction as other outp | PARTIAL | error states, context meter warning, stopped-reply notes; no retry row |
| B-W22 | Source/file viewer | ADAPT `@pierre/trees` and Pierre file view (Apache-2.0, credit in NOTICE) if CSP-clean; ot | PARTIAL | passage viewer with line numbers (row 15); no file tree |
| B-W23 | Diff viewer and review | ADAPT `@pierre/diffs` (check CSP and wasm first, NOT VERIFIED) or own unified renderer; EX | BUILT | fix diff viewer with numbers, signs, changed words (df01-df12, e2e) |
| B-W24 | Citations and source navigation | IMPROVE. Click a citation to open the file at that line with the quote highlighted, and sh | BUILT | quotes checked against files, passage opens in place, Copy location (row 15) |
| B-W25 | Attachments and drag-drop | ADAPT the classify/dedupe/track/overlay logic. IMPROVE: dropping a file is an import into  | PARTIAL | drop anywhere on the page, files-only drop target with Escape/leave, paste files, no duplicates (hash+name); OpenCode dropzone.tsx and attachments.ts read in full; no workspace-tree @mention drop (attachments.md) |
| B-W26 | Document library / indexing | BUILD workspace sources: local embedding in the kernel, per-file state (pending, indexed,  | PARTIAL | Documents reads the chosen folder per question; no persistent index |
| B-W27 | Artifacts and rich previews | EXCLUDE interactive artifacts for now: running model-written code in the page is the oppos | EXCLUDED | interactive artifacts not offered |
| B-W28 | Composer behaviour | ADAPT the reducer (pure TS, MIT) including the `composing` guard; fix the IME bug independ | BUILT | composer: IME-safe Enter, drafts per conversation, history, picker, Esc/Ctrl+G (cm01-cp24, e2e) |
| B-W29 | Slash commands and mentions | ADAPT; slash set matches the terminal workspace (`/review`, `/ask`, `/where`, `/memory`, ` | PARTIAL | slash popover in the web chat with the terminal's words (15 commands), Enter runs an exact command, /approve opens the page and approves nothing; OpenCode machine.ts read in full; no @ mentions, no in-conversation commands (web-shell.md) |
| B-W30 | Saved prompts and variables | BUILD later, vault-stored; low | LATER | saved prompts: low |
| B-W31 | Route and model chooser | IMPROVE. Chooser lists only routes the kernel allows for this workspace and shows class (l | LATER | one route today; a chooser matters once NEAR credits give a second route |
| B-W32 | Provider and credential management | BUILD a minimal key form that POSTs once to the companion, never echoes the key back, neve | LATER | needs the NEAR key and credits |
| B-W33 | Settings and settings search | BUILD a settings area; ADAPT the catalog-driven search index (pure data, MIT). Every setti | PARTIAL | settings screen with catalog-driven search (11 settings, both languages); no editable privacy preferences by design (web-shell.md) |
| B-W34 | Usage, cost and context meter | ADAPT gauge + breakdown (MIT). Data from kernel events for local routes (tokens, time) and | BUILT | context meter with the model's own count and window (mt01-mt06, sv49-sv52) |
| B-W35 | Approval mode indicator | ADAPT the chip as read-only status of the kernel policy (e.g. "ask for every release"). EX | BUILT | header badge Releases ask you (signed when an approval key is enrolled), status only, from /api/config (web-shell.md) |
| B-W36 | Permission and approval prompt | IMPROVE, not replace. Adopt the dock layout, a countdown showing expiry (`Release.expires` | BUILT | approval card shows time left (service clock), warns at 10 min, expired disables request and deliver; kernel already enforced; terminal view and Enter fail closed (web-shell.md) |
| B-W37 | Notifications | ADAPT in-app toast + tab-title count + optional Notification API. IMPROVE: notification te | BUILT | waiting count, desktop notices (n01-n14), e2e |
| B-W38 | Empty, loading, error states | BUILD error boundary with copyable, content-free diagnostics; ADAPT skeleton for lists. EX | BUILT | error boundary, loading and unreachable states (e2e) |
| B-W39 | Accessibility | ADAPT Kobalte Tabs, Dialog, Collapsible, Menu (MIT) instead of hand-rolled; ADAPT the anno | BUILT | axe checks on every screen in e2e; roving tabs; focus return |
| B-W40 | Responsive and mobile | ADAPT the drawer for sidebar and for the source/diff panes; test at 360 px. Phone pairing  | BUILT | 390 px mobile pass in e2e |
| B-W41 | Theming | IMPROVE small: add `prefers-contrast: more` and `forced-colors` support; keep tokens repla | BUILT | prefers-contrast more, forced-colors and reduced motion in the web app; tested with emulation and axe in light and dark (web-shell.md) |
| B-W42 | Internationalisation | ADAPT plural and RTL logic (MIT) and keep our parity tests as the gate for every new local | BUILT | English and Hindi with parity tests (i18n01-i18n03) |
| B-W43 | Offline and PWA | Do NOT add a service worker to the companion app. BUILD it for `/verify` only when it is s | LATER | verifier page offline use belongs with the recipient verifier |
| B-W44 | Export, import, delete | ADAPT filename sanitiser and format picker (markdown + JSON). IMPROVE: export is a kernel- | PARTIAL | notes export/import and web conversation export to the exports folder; no partial/other formats, no whole-profile export in the web (web-shell.md) |
| B-W45 | Sharing and multi-user | EXCLUDE public links, accounts, roles and people pickers (cloud and multi-tenant). The cap | EXCLUDED | no public links, accounts or roles |
| B-W46 | Original text versus what the model saw | ADAPT the concept and the isolation rule. IMPROVE: use the kernel's own redaction record ( | PARTIAL | notes used, summary and pages shown per answer (sv43, sv55); no raw-versus-sent view |
| B-W47 | Per-answer provenance panel | IMPROVE: a "What went into this answer" popover listing the notes (with why-retrieved), fi | BUILT | Notes used for this answer with why and source (sv43, e2e) |
| B-W48 | Memory UI | IMPROVE small: scope (workspace vs global), pin tier, "used in answers" count; keep the co | PARTIAL | project scope shown; merge and undo (me32-me40); no pin tier or used-in-answers count |
| B-W49 | Plan, todo and goal display | ADAPT later with the worker loop (`worker-loop.md`); a Build/Start button is a request to  | LATER | plan display waits for the worker loop |
| B-W50 | Undo of an applied change | BUILD through the effect ledger (`effect-ledger.md`): undo is a new, logged reverse effect | BUILT | apply and undo of a fix with guards (ff01-ff14) |
| B-W51 | Telemetry and third-party egress | EXCLUDE all telemetry, including opt-in. Make this a regression test, not a policy sentenc | BUILT | no telemetry; e2e checks that the page contacts only the local service |
| B-W52 | Voice input and output | EXCLUDE the browser speech-recognition API (in common browsers it can send audio to a vend | EXCLUDED | browser speech recognition can send audio to a third party |
| B-W53 | Skills, plugins, agents and prompt stores | BUILD later, behind the kernel broker only; EXCLUDE remote registries as an install source | LATER | skills and plugins behind the kernel broker only |

## Matrix C: Channels (Telegram)

| Key | Capability | Matrix decision | Status | Evidence or reason |
|---|---|---|---|---|
| C-1 | Direction of pairing | EXCLUDE the stranger-first flow (it makes the bot an oracle and sends text to unknown peop | EXCLUDED | EXCLUDE the stranger-first flow (it makes the bot an oracle and sends text to unknown people; OpenClaw `pairing-messages.ts` 6-27 even prints the owner's CLI co |
| C-2 | Code strength and storage | IMPROVE: reuse `kk-companion::Security::issue_pairing_code`/hash/lockout so there is ONE p | BUILT | one-time code, expires, replaced by a newer one (tg16) |
| C-3 | Attempt limiting | IMPROVE: per-chat budget (3) plus global lockout with persistence, uniform wrong/expired w | BUILT | closes after five wrong tries (tg16) |
| C-4 | Bind to stable principal | IMPROVE: store `chat_id` and `from.id`, require equality on every update, drop `from.is_bo | BUILT | one private chat bound; strangers get silence (tg01) |
| C-5 | Multiple chats / devices | BUILD later (low priority): up to 3 bound chats, each with id, label and paired-at, listed | LATER | low priority |
| C-6 | Revocation and unpair | IMPROVE: bridge re-reads `telegram.json` every poll cycle (or the service owns binding sta | BUILT | `kaku telegram unpair`; the bound chat is re-read (code in telegram.rs; no named test: NOT VERIFIED) |
| C-7 | Pairing UX | IMPROVE: call `getMe` at start to learn the username, print and QR the deep link; handle ` | PARTIAL | kaku telegram pair prints the bot's t.me link from getMe, no link on any failure; no QR code; live bot NOT VERIFIED (telegram-bridge.md) |
| C-8 | Unauthorised-sender reply policy | Keep. EXCLUDE hints and declines | BUILT | silence for strangers (tg01) |
| C-9 | Owner vs member roles | EXCLUDE (no write powers exist to scope) | EXCLUDED | EXCLUDE (no write powers exist to scope) |
| C-10 | Polling vs webhook | Keep polling for local mode (no public endpoint, which is the whole local-first point). AD | KEPT | polling, no public endpoint |
| C-11 | Real long poll | IMPROVE: `timeout=25`, client timeout 35; keep a separate notification tick so event laten | BUILT | poller thread with 25 s long poll, 35 s client timeout; notices unaffected (2 s); fake-server measured; real Telegram NOT VERIFIED (telegram-bridge.md) |
| C-12 | Reconnection backoff | IMPROVE: 2 s start, x2, cap 60 s, jitter, reset on success. OpenClaw's 30 s start is too s | PARTIAL | polling errors back off 2 s doubling to 60 s with jitter (tg61); OpenClaw restart policy read FULL: its start is 30 s, ours differs by decision (telegram-bridge.md) |
| C-13 | 409 conflict and stale webhook | ADAPT the behaviour: `deleteWebhook` at start, on 409 re-clear and back off, after N tries | PARTIAL | 409 reported and retried; deleteWebhook only when asked in doctor (code, no named test) |
| C-14 | Token rejected (401/404) | BUILD: `getMe` at start; 401 -> stop retrying, report `blocked: token rejected` in health  | BUILT | 401 stops the bridge with a clear message; doctor getMe (code; no named test) |
| C-15 | Stall watchdog | ADAPT in small form: a hung request is already bounded by the client timeout; add "no succ | KEPT | client timeouts already bound a hung request |
| C-16 | Update dedupe and offset | ADAPT Hermes's receipt-set idea (bounded, 24 h TTL), not a watermark. Needed only if any i | PARTIAL | offset advanced; no receipt set (low risk: replies are idempotent) |
| C-17 | Allowed updates | Keep. Revisit only if non-consequential buttons are added (3.6) | BUILT | allowed_updates is message only |
| C-18 | Network quirks | BUILD small: honour `HTTPS_PROXY`; document; no DNS tricks | PARTIAL | local and pinned clients never use an environment proxy (a real privacy defect found and fixed); Telegram keeps the library default that honours HTTPS_PROXY, not tested end to end (proxies.md) |
| C-19 | Typed send outcome | ADAPT: port the five-way outcome enum (IronClaw is MIT/Apache Rust; take the status mappin | BUILT | classify_send and read answers (tg17, tg25) |
| C-20 | 429 flood control | ADAPT: gate with deadline; short waits (<= 5 s) inline, longer ones go back to the outbox  | PARTIAL | retry_after honoured in the outbox (tg17) |
| C-21 | Durable outbox (delivery ledger) | ADAPT the design (Python, MIT; rebuild in Rust, store as a small SQLite/JSON file in the d | PARTIAL | bounded outbox with backoff (tg17); in memory, not kept across a restart |
| C-22 | Notification replay after downtime | IMPROVE with a rule: on start, announce only items **still** pending (an approval still wa | PARTIAL | never announces on the first look (tg03); catch-up rule after downtime not tested |
| C-23 | Message size and splitting | Keep the cap; ADAPT IronClaw's `chunk_text_utf16` only if a digest can exceed it. Add "+N  | BUILT | long replies are cut (tg05) |
| C-24 | Formatting | Keep plain. EXCLUDE markdown/HTML (every markup mode is an injection surface for ids and a | BUILT | plain text only |
| C-25 | Per-chat pacing | BUILD small: at most 1 message/s per chat, burst of 3; digest when more than 5 events arri | PARTIAL | outbox flood gate for the whole bot and a three-message burst then one a second; digest of many events not built; OpenClaw throttler read FULL (telegram-bridge.md) |
| C-26 | Quiet hours and mute | BUILD small: `/mute 1h` (bridge-local state, cannot widen access), `disable_notification=t | PARTIAL | /mute 30m/2h/off in the bound chat: notices arrive without sound, none held back; reference sources NOT READ, live Telegram NOT VERIFIED (telegram-bridge.md) |
| C-27 | Typing and ack reactions | EXCLUDE (cosmetic; reactions need extra update types) | EXCLUDED | EXCLUDE (cosmetic; reactions need extra update types) |
| C-28 | Command set and menu | BUILD small: `setMyCommands` for exactly those commands at start (idempotent) so the phone | BUILT | menu lists exactly the commands answered (tg21) |
| C-29 | Chat as agent (ask, review, remember) | EXCLUDE for now. If ever added: inbound text is untrusted data, carries Ghast-style proven | EXCLUDED | EXCLUDE for now. If ever added: inbound text is untrusted data, carries Ghast-style provenance (concept only), runs a chat with no tools, and its **answers** wo |
| C-30 | Session / conversation mapping | EXCLUDE (nothing to map). Keep a `chat -> binding` record only | EXCLUDED | EXCLUDE (nothing to map). Keep a `chat -> binding` record only |
| C-31 | Cancellation | EXCLUDE `/cancel` (a write). Reconsider only as "request cancel" that the service confirms | EXCLUDED | EXCLUDE `/cancel` (a write). Reconsider only as "request cancel" that the service confirms on the trusted surface |
| C-32 | Per-chat inbound serialisation and flood | BUILD small: at most 5 commands per 10 s from the bound chat, then silence | BUILT | five commands per ten seconds (tg20) |
| C-33 | Group / topic / channel behaviour | Keep. EXCLUDE groups: a group is many principals and a shared history | KEPT | Keep. EXCLUDE groups: a group is many principals and a shared history |
| C-34 | Approval over chat | **EXCLUDE chat approval entirely.** Every reference puts the action detail (command, param | EXCLUDED | **EXCLUDE chat approval entirely.** Every reference puts the action detail (command, parameters, sometimes file text) into the chat provider and lets a chat mes |
| C-35 | Approval *lifecycle hygiene* (terminal receipt, expiry, stale button) | IMPROVE (this part is safe): keep the message id of an "approval waiting" ping and, when t | BUILT | stale approval notice corrected once, edit only for own notice (tg24, tg26) |
| C-36 | Trusted-device handoff | IMPROVE the text only: include the review id and the exact surface ("Kakurega app, Approva | BUILT | approval notice names the review id and where to decide (workspace 'Waiting for you' or kaku approve); places verified to exist (telegram-bridge.md) |
| C-37 | Non-consequential buttons | Optional later, tightly bounded: "Mute 1 h" and "Unpair this chat" buttons whose callbacks | LATER | optional, bounded |
| C-38 | Inbound media/voice | EXCLUDE. Files from a chat are untrusted input into the user's machine for no benefit to a | EXCLUDED | EXCLUDE. Files from a chat are untrusted input into the user's machine for no benefit to a counts-only bridge |
| C-39 | Outbound media/files | EXCLUDE (would carry content) | EXCLUDED | EXCLUDE (would carry content) |
| C-40 | Inbound attachments' captions, forwards | EXCLUDE | EXCLUDED | EXCLUDE |
| C-41 | Health/status | BUILD: bridge writes `telegram-status.json` (0600) every 30 s: running, last poll ok, last | BUILT | status file alive/stale/never, no text or token (tg23) |
| C-42 | Doctor / audit | BUILD: `kaku telegram doctor`: token file is regular, not a symlink, mode 0600; `getMe` ok | BUILT | `kaku telegram doctor` |
| C-43 | Token handling | IMPROVE: wrap every Telegram call in one function returning an error type that never conta | BUILT | token never in error text (tg04) |
| C-44 | Secret storage | IMPROVE: use the existing vault for the bot token if the vault is the single secret store  | PARTIAL | token file must be a regular owner-only file (tg22); not in the vault |
| C-45 | Test harness | BUILD: a Rust fake Bot API (axum or tiny_http, dev-dependency) with inject/drain/fail-next | BUILT | fake Bot API in the bridge tests (tg01-tg26) |
| C-46 | Slack, Discord, Matrix, Signal, SMS, WhatsApp adapters | BUILD later behind one `Transport` trait **inside the bridge**, not a second kernel. Prior | LATER | other chat adapters |
| C-47 | Linked personal account (MTProto) | EXCLUDE: it reads the user's private chats; the opposite of the product | EXCLUDED | EXCLUDE: it reads the user's private chats; the opposite of the product |
| C-48 | Multi-bot / multi-account routing | EXCLUDE | EXCLUDED | EXCLUDE |

## Matrix D: MCP and agent access

| Key | Capability | Matrix decision | Status | Evidence or reason |
|---|---|---|---|---|
| D-D01 | Lifecycle and version negotiation | ADAPT via SDK. Restrict `supported_protocol_versions` to 2025-06-18 and later. | BUILT | the client's protocol version is echoed when supported (mcp.rs; mc tests) |
| D-D02 | Capability declaration | ADAPT via SDK (`ServerCapabilities::builder()`). Declare only what exists. | BUILT | tools only; listChanged false (mcp.rs) |
| D-D03 | Server `instructions` | KEEP static. EXCLUDE any dynamic content, ever. | BUILT | static text (mcp.rs) |
| D-D04 | Server identity, titles, icons | ADAPT via SDK; add a small PNG `data:` icon. | PARTIAL | name, title and version; no icon |
| D-D05 | Error model | IMPROVE; SDK does it. | BUILT | typed JSON-RPC errors; unknown tool, bad params, too large (mc tests) |
| D-D06 | Message size and framing | IMPROVE: SDK with a 1 MiB cap; wrap the transport to emit `-32600` for the dropped id (ext | BUILT | bounded line reader with a size error (mc tests) |
| D-D07 | Concurrency, ordering, timeouts | ADAPT via SDK; set explicit connect and read timeouts (for example 10 s and 30 s) on the ` | BUILT | running-call registry, duplicate ids refused, drain at end of input (mc18-mc21) |
| D-D08 | Cancellation | ADAPT via SDK, plus wire the token into the admin HTTP call. | BUILT | cancel reaches only a running call (mc18-mc21) |
| D-D09 | Progress | BUILD on top of SDK for `review_wait`, using the job's existing `events` (stage names). | PARTIAL | progress is available by polling review_events; no MCP progress notifications or blocking review_wait (mcp.md) |
| D-D10 | Pagination and cache hints | ADAPT via SDK. Mark every `resources/read` `private` with `ttlMs: 0`. | LATER | no resources yet |
| D-D11 | Logging capability | EXCLUDE. Log to stderr only, structured, no payloads. | EXCLUDED | logs go to stderr only |
| D-D12 | Completion | BUILD later, only for review ids. | LATER | only useful with prompts and resources |
| D-D13 | Ping | ADAPT via SDK. | BUILT | ping answered |
| D-D14 | Trace context | BUILD small: copy the trace id into the signed log entry for the call. | BUILT | well-formed traceparent from params._meta kept in the signed mcp.call entry, malformed dropped; real-client sending NOT VERIFIED (mcp.md) |
| D-D15 | Tool schema design and names | KEEP. Keep names at most 24 characters and ASCII `[a-z_]` only (no dots) so Cline and Open | BUILT | names a-z and underscore, short |
| D-D16 | Input validation | IMPROVE: keep Kakurega's rules as validated newtypes (`ReviewId`, `RecipientName`, `Folder | BUILT | schemas forbid extra fields; check_args |
| D-D17 | Structured output and `outputSchema` | IMPROVE now (section 5), then SDK typed outputs. | BUILT | outputSchema on every tool, replies checked against it (conforms) |
| D-D18 | Annotations | KEEP; add a test that ties each annotation to behaviour. | BUILT | readOnlyHint and friends follow MUTATING (mcp_tools.golden.json) |
| D-D19 | Tool execution errors | KEEP; whitelist the forwarded message set instead of passing `v["error"]["message"]` throu | BUILT | forwarded error text whitelisted by code and plain-sentence check; fe01, 8 mutants (mcp-clients.md) |
| D-D20 | Catalog stability and rug-pull defence | IMPROVE: expose a catalog digest in `server/discover` `_meta` (`com.kakurega/catalogDigest | BUILT | catalog digest in initialize and tools/list _meta and `kaku mcp --print-catalog-digest`; listed digest for filtered lists (mc42) |
| D-D21 | Tool profiles and allowlists | BUILD: `kaku mcp --profile observer` (overview, list, status, verify), `--profile reviewer | BUILT | --read-only, --allow and --leased (mc06, mc15-mc17, mc22-mc29) |
| D-D22 | New tools: a path for every safe capability | BUILD in tiers. | LATER | tiers exist; more tools when worker loop arrives |
| D-D23 | `resource_link` and embedded resources in results | BUILD: return `resource_link` for verification reports and capsule files, never embedded c | BUILT | verify_capsule and verify_log hand over resource_link to a kept report, never embedded; read through resources/read, newest eight kept (mc43, mc44) |
| D-D24 | Resources (list, read) | BUILD under a custom scheme `kakurega://` with read-only resources: `kakurega://overview`, | BUILT | `kakurega://overview` resource, gated like the tool, audited as `overview` (mc35-41, live 51/51) |
| D-D25 | Resource templates | BUILD with D24: `kakurega://review/{id}/status`. | BUILT | `kakurega://review/{id}/status` template; only the exact shape resolves (mc38, 17 hostile addresses) |
| D-D26 | Prompts | BUILD static prompts, no data: `private_review` (walks through start, wait, request approv | BUILT | `private_review` prompt, offered only while its three tools are allowed, values quoted (mc40, mc39) |
| D-D27 | Change notifications and subscriptions | BUILD both: a cursor feed `review_events(after, limit)` over job events (any client works) | PARTIAL | review_events cursor feed over review events (stage names only), positions kept across restarts; no resource subscriptions (mcp.md) |
| D-D28 | Long-running tasks | BUILD on Kakurega's own job store: a task is a view of a persisted job. Use the SDK only f | LATER | start_review returns an id; tasks protocol not used |
| D-D29 | Elicitation, multi round-trip requests (MRTR) | EXCLUDE as an approval or consent channel. Optional later: a URL-mode elicitation that onl | EXCLUDED | not an approval channel |
| D-D30 | Sampling | EXCLUDE. | EXCLUDED | not offered |
| D-D31 | Roots | EXCLUDE as authority. Folders stay governed by the owner's allowed roots (`allowed_dir`, V | EXCLUDED | folders stay with the owner's allow list |
| D-D32 | Authorization and transports | BUILD: (a) a scoped agent token enforced by the existing companion guard, not a second aut | PARTIAL | stdio only; owner leases scope the acting tools (agent-leases.md); no token transport |
| D-D33 | Rate and size limits | BUILD: per-token budget (for example 60 calls per minute, 4 `start_review` per hour, respo | PARTIAL | use and time limits per lease (sv63-sv67); no per-minute budget |
| D-D34 | Extensions: skills over MCP, apps | BUILD later: one static skill `kakurega-safe-review` so agents learn the flow. EXCLUDE App | LATER | static skill for agents |
| D-D35 | Audit and evidence for MCP calls | BUILD: every call becomes a signed-log event `mcp.call` with tool, token id, argument dige | BUILT | every tool call is a signed `mcp.call` log event with the tool and outcome only; floods are counted (sv75, sv76, mc30-mc33, live 47/47) |
| D-D36 | Testing and conformance tools | ADAPT: add conformance run and Inspector CLI smoke to CI; keep the canary scan; add the sc | BUILT | official-SDK client interop, framing tests, live run (mc tests, mcp_live.py 43/43) |
| D-D37 | Client compatibility quirks (facts to design around) | ADAPT: keep a `docs/mcp-clients.md` table and a test that replays three recorded client ha | BUILT | docs/mcp-clients.md + mcp_clients.rs mk01-mk03 replay six client openings built from their source (not captured from running clients), 5 mutants caught |
| D-D38 | Installing the server into a client | BUILD `kaku mcp install --client NAME --print` printing the exact command, binary path and | BUILT | `kaku mcp install --client NAME --print`: full path, limits, no token, digest on stderr, never writes (mi01-mi08, live 52/52) |
| D-D39 | Hosted and remote MCP | Defer to hosted mode; design D32 so the scope names equal OAuth scopes. | LATER | hosted mode |
| D-D40 | Identifiers and cross-call state | IMPROVE with D28: keep `job-N` for humans, add an opaque handle for MCP and check principa | LATER | job-N ids stay |

## Matrix E: Memory, agent, coding

| Key | Capability | Matrix decision | Status | Evidence or reason |
|---|---|---|---|---|
| E-M1 | Extraction from conversations | ADAPT idea (ADD-only, id aliasing, date grounding); BUILD the pass so its output always la | PARTIAL | owner-requested reading of a kept conversation: suggestions only, injection/secret filter, dedupe, project scope; web only; real-model quality NOT VERIFIED (memory.md) |
| E-M2 | Exact dedupe | keep | KEPT | fingerprint on write |
| E-M3 | Near-duplicate / paraphrase dedupe | ADAPT algorithm (Apache), output = "possible duplicate" proposals for the owner | BUILT | look-alike report (row 26) and owner-confirmed merge with undo (me32-me40, sv70-sv74) |
| E-M4 | Update / contradiction handling | IMPROVE: add `conflicts_with` proposals (same subject, overlapping validity); ADAPT Hermes | BUILT | disagreeing notes: same-subject and changed-claim readings, keep one with history or both right; web, service, terminal; measured 12/12 found, 0/13 unrelated flagged, look-alikes listed (memory.md) |
| E-M5 | Temporal validity | keep; IMPROVE: add "expires soon" listing, and `observed_at` separate from `created_at` | PARTIAL | expiring-soon list in memory and /api/memories/expiring; no screen for it, no observed_at (memory.md) |
| E-M6 | Lexical retrieval | IMPROVE (porter/lemma tokeniser; keep the injection-proof query builder) | BUILT | BM25 and similar-wording rankings, injection-proof query (me tests); no stemmer |
| E-M7 | Candidate pool before filtering | IMPROVE now: put level/compartment in the SQL `WHERE` (columns exist: `level`, `compartmen | PARTIAL | level and project are in the SQL before ranking; compartments are checked after ranking (recall in kk-memory) |
| E-M8 | Hybrid (vector + keyword) retrieval | ADAPT RRF (simple, rank-based, no score calibration; Apache). Vectors must be produced loc | BUILT | three rankings merged by rank fusion, measured on LongMemEval (memory.md) |
| E-M9 | Reranking | ADAPT the "only over a cap" rule; reranker runs locally or on a TEE route | LATER | only over a cap; needs a model route |
| E-M10 | Entity / graph boost | BUILD later, low priority; the label problem is that an entity index can leak existence of | LATER | low priority |
| E-M11 | Context packing | IMPROVE: query-relevant recall, token budget, diversity cap; ADAPT packer logic | BUILT | notes get a third of the window, whole notes only (sv52); recall by query |
| E-M12 | Scoping / namespaces | BUILD `scope` column (project id) distinct from compartments: scope decides relevance, lab | BUILT | project scope (me tests, row 27) |
| E-M13 | Retention / decay | ADAPT formula for RANKING only; any archiving is a proposal the owner accepts (Hermes rule | PARTIAL | freshness nudge in recall (<=5% of score, half-life 90/180 days, log-scaled use records written only when notes reach the model); no dormant/archive/pin; half-lives untuned (memory.md) |
| E-M14 | Memory provenance and origin class | IMPROVE: add `origin` closed set; mark memory text injected into a review so `suggest_memo | PARTIAL | source and untrusted flag shown; no origin set |
| E-M15 | Promotion gate | keep owner gate; ADAPT bounded-loss + hash-recheck if a consolidation pass is ever built | KEPT | owner gate |
| E-M16 | Secret and instruction detection | IMPROVE: when detectors change, rescan the store and quarantine hits (hide from recall, sh | PARTIAL | held status + rescan on detector generation change (pinned by corpus fingerprint), owner decides in web Memory/service; no terminal view, instruction-like not rescanned (memory.md) |
| E-M17 | Owner-gated writes by background agents | keep; ADAPT pin-on-stage for corrections | KEPT | suggestions need the owner |
| E-M18 | Deletion guarantee: our own store | IMPROVE now: blank `proposals` rows whose fingerprint equals a forgotten fingerprint, and  | BUILT | forget deletes text, vectors, blanks matching proposals, blocks the wording (me tests) |
| E-M19 | Deletion guarantee: references | IMPROVE: (a) encrypt the backup with a passphrase, (b) forget returns a list "copies this  | PARTIAL | forget lists known copies by name (reviews, exports) and always what it never reaches; web shows it; kept conversations not searched, per-copy removal not built (memory.md) |
| E-M20 | Lineage purge | BUILD `forget_by_source(review_job)` and a "forgotten source" tombstone so a re-run of tha | BUILT | forget_source + footprint + tombstoned source; web, service, terminal; lineage forward and back (memory.md) |
| E-M21 | Tombstone against resurrection | keep (ours is better: normalised and keyed) | KEPT | normalised and keyed |
| E-M22 | Import / export / backup | IMPROVE: encrypted backup, carry label, validity, source and tombstones; imported text pas | BUILT | encrypted backup with recovery key, notes export and import (backup tests) |
| E-M23 | Explainability | IMPROVE small: show matched terms and rank | PARTIAL | matched words and rank returned and in the why line; no highlight in the web list, none for meaning-only matches (memory.md) |
| E-M24 | Procedural memory / skills | BUILD later as owner-approved "playbooks" (a skill is executable influence, so same propos | LATER | owner-approved playbooks |
| E-M25 | Telemetry | EXCLUDE (violates the privacy goal) | EXCLUDED | none |
| E-M26 | Evaluation sets | BUILD our own small suites: recall@k with labels, as-of correctness, forget completeness ( | PARTIAL | LongMemEval recall harness (recall_bench.rs); no as-of or correction suites |
| E-A1 | Event-sourced state, checkpoint after every step, resume | keep | KEPT | job store resumes |
| E-A2 | Context compaction | BUILD in the OpenHands shape (tombstone event, View) with Hermes pieces (verbatim user tex | BUILT | summaries with data labelling and a wait after failure (sv55, sv56, sv68) |
| E-A3 | Large tool output handling | ADAPT idea: spill to a labelled artifact handle, never to /tmp (plaintext) | LATER | worker loop |
| E-A4 | Stuck / loop detection | ADAPT (MIT/Apache) at the worker, nudge once then end the run with a visible reason | PARTIAL | worker stuck detector: repeated and alternating responses, told once then stopped with state saved; OpenHands detector read FULL; real-model effect NOT VERIFIED (worker-loop.md) |
| E-A5 | Tool-call recovery | IMPROVE: bounded retry with backoff for transient model errors, never for policy denials | BUILT | bounded retry with backoff for busy or unreachable local models, never for refusals, bad requests, timeouts or mid-stream failures (model-retry.md) |
| E-A6 | Budgets | keep | KEPT | budget.rs |
| E-A7 | Subagents | BUILD later as separate sandboxed workers under the same broker with their own labels and  | LATER | separate sandboxed workers |
| E-A8 | Planning / todo | NOT READ enough to decide; revisit after A2 | LATER | after the worker loop |
| E-A9 | Skills and review checks as files | BUILD owner-authored checks; they are instructions to a model, so they are first-class tru | LATER | owner-authored checks |
| E-A10 | Critic / second opinion | EXCLUDE remote critic; optional local-model critic stays behind the same label rules | EXCLUDED | no remote critic |
| E-C1 | Patch format | IMPROVE: allow several hunks and several files in one fix, each exactly-once; ADAPT the Co | BUILT | several changes per file joined into one exact change (exact-once with overlap-aware counting, no shared text, bounded span); real line diff with separate hunks; apply_all refuses duplicate files; fix-format.md |
| E-C2 | Fuzzy matching | IMPROVE narrowly: allow OpenCode-style normalised fallback only when it yields exactly one | PARTIAL | aligned matching: exact first, then look-alike characters and CRLF, then ends of lines, only when exactly one place; file's own text applied and shown; OpenCode edit.ts read; real-model effect NOT VERIFIED (fix-format.md) |
| E-C3 | Two-phase apply (compute, then write) | IMPROVE: per-path lock and re-check hash after taking it (read-to-rename window is lock-fr | BUILT | logged, flushed, re-read, per-review lock (ff01-ff14) |
| E-C4 | Snapshots, checkpoints, undo | BUILD: before apply, store the original bytes in the vault (encrypted, labelled) keyed by  | BUILT | originals stored, undo guarded (ff, sv24) |
| E-C5 | How a patch is judged correct | IMPROVE: add a second oracle = the project's own tests/linter run in the sandbox before an | BUILT | reproducer plus the project's own tests (verify_fix_with_tests) |
| E-C6 | Running tests and linters safely | BUILD: test/lint runner in `kk-sandbox`, no network, project read-only, scratch writable,  | BUILT | in the sandbox, no network |
| E-C7 | Language coverage | BUILD closure per language (start JS/TS and Rust); ast-grep (MIT, per project standard) fo | PARTIAL | Python and JavaScript; TypeScript and Rust owed |
| E-C8 | Withheld-file handling | keep | KEPT | withheld files never leave |
| E-C9 | Error messages that quote file text | ADAPT the rule: any hint quoting source goes through the label check for the viewer | PARTIAL | audit found no message quoting source or program output beyond a validated package name; name-based guard script added to scripts/test; OpenCode edit.ts block read (fix-format.md) |
| E-C10 | Diff review UX | IMPROVE: real unified diff with context using `similar` (Apache-2.0, add to NOTICE); side- | BUILT | fix diff viewer (row 19) |
| E-C11 | Permission gating of edits | keep; explicitly do NOT adopt a second rule engine (policy stays in `kk-policy`) | KEPT | one policy engine |
| E-C12 | Repo mapping / context building | Study before deciding; candidate: tree-sitter symbol map built locally, labelled per file | LATER | study before deciding |
| E-C13 | Reproducer-first repair loop | IMPROVE order: let the model re-propose after `StillReproduced`/`CouldNotApply` feedback ( | PARTIAL | reproducer-first; re-proposal after a still-reproduced result not looped |

## Matrix F: Desktop, documents, security, tooling

| Key | Capability | Matrix decision | Status | Evidence or reason |
|---|---|---|---|---|
| F-S1 | Provider-specific token rules | ADAPT the specific-format rules (not the toml verbatim): port rule set to a single Rust ta | BUILT | about 130 layouts as table entries (secret-detection.md; formats tests) |
| F-S2 | Generic keyword=value rule with allowlists | IMPROVE: keep our value-shape check but add detect-secrets-style filters (below) and gitle | BUILT | keyword assignments with false-positive filters (context tests cx01-cx15) |
| F-S3 | Heuristic false-positive filters | ADAPT (behaviour re-implemented in Rust; attribution) | BUILT | placeholders, UUIDs, hashes, member paths, test markers (secret-detection.md) |
| F-S4 | Entropy with charset awareness | IMPROVE: exclude pure hex of length 32/40/64 and UUID shape unless a keyword is adjacent;  | BUILT | entropy plus format and placeholder rules |
| F-S5 | Decoded-secret detection | ADAPT behaviour: decode candidate segments (bounded depth and size) before scanning, in im | BUILT | base64, hex, percent, unicode escapes, bounded depth (sk03) |
| F-S6 | Release egress check against known withheld values | IMPROVE (highest value in this document): at release assembly, for each Restricted value a | BUILT | values of withheld files and their encodings are refused at release (rs08, rs09) |
| F-S7 | Independent second detector at egress | ADAPT: run the S1 table (our single Rust table) over every released file in addition to S6 | BUILT | the release check uses the same table as import (one crate); an independent second detector is not run |
| F-S8 | User-declared withhold list | BUILD `.kakuignore`-style file honoured by the importer (labels Restricted, same path as ` | BUILT | `.kakuignore` honoured by the importer, fail closed (withhold-list.md; ki01-ki07; live review 3 withheld) |
| F-S9 | Crypto-wallet secrets (NEAR relevance) | BUILD (nothing to reuse): `ed25519:<base58 88>` private keys, BIP-39 word runs checked aga | BUILT | NEAR and Solana key formats and a recovery-phrase shape rule; no BIP-39 word list yet |
| F-S10 | Live validation of found credentials | EXCLUDE: validation sends the secret to a third party, breaking the privacy promise | EXCLUDED | sending a secret to a third party |
| F-P1 | Pattern + checksum + context recognisers | ADAPT the checksum/invalidation logic and score-plus-context model into Rust for cards, IB | BUILT | Luhn and Verhoeff in kk-secrets pii.rs (tests pii) |
| F-P2 | Name/address detection (NER) | IMPROVE later: only through a local model, or the TEE endpoint after the existing route/at | LATER | needs a local or attested model |
| F-P3 | PII allowlist owned by the operator | ADAPT idea: owner-controlled list recorded in the manifest (an entry is evidence in the ca | PARTIAL | owner allowlist for the personal-data notice: /api/pii-allow + /pii, strict entries, counted not silent; Privateer piiAllow read in full; no web editor, capsule does not carry the list (secret-detection.md) |
| F-P4 | Silent auto-redaction | EXCLUDE: a bypass of approval; false positives become silent rewrites the model reads (Pri | EXCLUDED | would bypass approval |
| F-I1 | Value-level capabilities with readers | IMPROVE inside the same lattice: add an optional per-compartment recipient allow-set that  | LATER | label lattice already in place; per-compartment readers later |
| F-I2 | Quarantined parsing | BUILD option: for documents, parse with the deterministic Docling path and give the agent  | LATER | offline-parsing.md; sandboxed parser option not adopted |
| F-I3 | Injection detection | IMPROVE: one shared normalised matcher (confusables, leet, whitespace, paraphrase patterns | BUILT | shared `kk_types::injection` reader: disguises, other languages, hidden and smuggled text, chat markup; special tokens removed at the route boundary; measured on 7,945 real documents (injection-reader.md) |
| F-I4 | Evaluation harness | ADAPT harness shape ("KakuDojo"): N repos x M injection goals (read `.env`, send to other  | PARTIAL | attack suites exist; no repo-by-goal grid |
| F-I5 | Deterministic exfil grading with canaries | ADAPT: plant per-run canaries in Restricted files, grade by canary presence (S6 variants)  | PARTIAL | canary checks in vault and service tests; no per-run planted canaries in Restricted files |
| F-I6 | Red-team plugin library | ADAPT as test generators for the existing attack suite (offline, local model or fixtures); | LATER | test generators |
| F-I7 | Independent ground truth for findings | ADAPT as an optional external signal: run `semgrep` as a subprocess on the sandboxed copy, | PARTIAL | optional semgrep second opinion (KAKU_SEMGREP): own rules, untrusted-input handling, shown per finding as pattern match not reproduction; real semgrep run by hand (fixture: 0 matches); not in the gate; no web view (pattern-rules.md) |
| F-D1 | Layout-aware PDF/DOCX/XLSX/HTML parse with anchors | ADAPT as an optional sandboxed parser process (Python is a second runtime: run it inside t | LATER | sandboxed parser option |
| F-D2 | Page anchors survive extraction | EXCLUDE for adoption; keep as the cautionary case. Sandboxed Docling path above is better  | EXCLUDED | cautionary case, not adopted |
| F-D3 | OCR | ADAPT: OCR through Docling inside the sandbox; label OCR text lower confidence; never feed | LATER | inside the sandbox later |
| F-D4 | Remote-fetch hazards in parsers | Keep network denied at the sandbox, plus `enable_remote_services=false` as belt and braces | KEPT | network denied in the sandbox |
| F-D5 | Import limits | KEEP; add depth/total-bytes limit and an archive policy (zip bomb) when archives are accep | KEPT | size and count limits exist; depth and archive policy owed (NOT VERIFIED) |
| F-E1 | 2, 3, 4: no Node in renderer, context isolation, sandbox | KEEP | KEPT | context isolation and sandbox on |
| F-E2 | 5: permission request handler | IMPROVE: also `setPermissionCheckHandler` per origin, `setDevicePermissionHandler`, and an | BUILT | permission, check and device handlers refuse (desktop-shell.md update) |
| F-E3 | 7: Content-Security-Policy | Re-verify in the packaged app | BUILT | CSP blocks inline script, eval, beacons (desktop attack suite 23/23) |
| F-E4 | 13, 14, 15: navigation, new windows, `openExternal` | KEEP; add `will-redirect` and `will-attach-webview` (present) review | BUILT | navigation, new windows, will-redirect refused (desktop attack suite) |
| F-E5 | 17: validate IPC sender | KEEP; if IPC is added, port per window with frame-origin check (record already says so) | KEPT | no IPC added |
| F-E6 | 18: avoid `file://`, use a custom protocol | IMPROVE: custom scheme or loopback with per-launch secret in a header added by the main pr | BUILT | per-launch shell secret on stdin, 403 SHELL_ONLY for every other program; pairing lockout no longer blocks the right code (ws13b/c, ws20-22, Electron e2e) |
| F-E7 | 19: fuses | IMPROVE: review `LoadBrowserProcessSpecificV8Snapshot` and `EnableEmbeddedAsarIntegrityVal | BUILT | fuses and asar integrity with a tested tamper response (II-13b, II-13c) |
| F-E8 | 16: current Electron | BUILD update policy | PARTIAL | update policy read from Electron's docs; scripts/check_electron.py (offline-tested logic); pin moved 45.2.2 to 45.2.3 and desktop suites green; no schedule, release notes unread (desktop-shell.md) |
| F-E9 | Single instance lock | ADAPT from OpenCode (MIT) | BUILT | second launch exits (desktop-shell.md) |
| F-E10 | Signing / notarization / hardened runtime | BUILD Developer ID signing + notarization + hardened runtime entitlements; OpenCode's buil | LATER | needs an Apple Developer ID; deferred by the owner |
| F-E11 | Auto-update with signature verification | BUILD an updater that verifies the signature and a pinned publisher; do not copy OpenCode' | PARTIAL | signed announcements with rollback and expiry (row 25); no installer step until signing exists |
| F-E12 | Renderer crash/unresponsive recovery | ADAPT OpenCode `windows/recovery.ts` idea (relaunch, export logs, keep waiting) with logs  | BUILT | companion restart and window reload with limits (ds10-ds11, desktop_recovery 7/7) |
| F-E13 | Native approval UX | KEEP; ADOPT Cline's two safe behaviours: auto-deny orphaned approvals when no UI is attach | KEPT | native approval |
| F-E14 | Two paths in `approval.mjs` | Keep disclosed; consider making an enrolled key mandatory for packaged builds | PARTIAL | disclosed; enrolled key not yet mandatory in packaged builds |
| F-E15 | Tauri alternative | Decision stays the owner's (II-04). Not re-argued here | LATER | owner's decision (II-04) |
| F-X1 | Minimal permissions | KEEP | KEPT | extension permissions minimal |
| F-X2 | Secret scan before sending | IMPROVE: run the shared S-table in the extension before POST too; both sides must agree | BUILT | same provider table in the extension (xs01-xs03) |
| F-X3 | Origin pinning | KEEP | KEPT | pinned origin |
| F-X4 | Port discovery | IMPROVE with native messaging? EXCLUDE: Ghast-style `nativeMessaging` widens the permissio | EXCLUDED | no native messaging |
| F-X5 | Page content capture beyond a selection | Not adopted; any addition needs `activeTab` and an explicit click. Decide only after readi | EXCLUDED | selection only |
| F-X6 | MV3 hardening review | Search and read Chrome extension security guidance before store submission | PARTIAL | Chrome MV3 security guidance read (two pages, partly) and the extension checked item by item against it, all already enforced by tests ex06-ex09; rest of Stay secure and store policies not read (browser-extension.md) |

## Matrix G: Ghast whole product

| Key | Capability | Matrix decision | Status | Evidence or reason |
|---|---|---|---|---|
| G-GH-01 | Five skippable first-run steps in about two minutes: language, invite  | IMPROVE: add language, name and theme steps, each skippable, plus the readiness screen of  | PARTIAL | first-visit welcome: language, theme, setup summary, each skippable, Escape skips, remembered; no name/invite/wallet by design; unaided-stranger test needs a person (web-shell.md) |
| G-GH-02 | Account is a wallet; "Create one for me" makes a seed phrase and an ac | EXCLUDE: a chain wallet as identity is a funds risk and not our model (project rule on see | EXCLUDED | a chain wallet as identity is a funds risk |
| G-GH-03 | First chat is blocked by a "Top up 0G funds" dialog | KEEP (their weakness, our opening). Do not add any gate before the first local review | KEPT | no top-up gate before the first chat |
| G-GH-04 | Sign-in by email code or Google, then onboarding; vault key issued by  | EXCLUDE a server-issued key (a backend that issues the key defeats local-first). Hosted mo | EXCLUDED | no server-issued key |
| G-GH-05 | Profiles: each owns a wallet, vault key, database, bridge credentials, | BUILD: profile = separate vault key file, separate SQLCipher database, separate policy and | LATER | separate profiles with their own vault keys |
| G-GH-06 | Workspaces/Projects bound to chats and channels; sidebar Projects and  | BUILD a project entity (folder, label policy, memory scope, history); ties to memory scope | PARTIAL | projects scope notes and chats; no project object with its own label policy |
| G-GH-07 | 11 interface languages | IMPROVE: move strings out of components first (`desktop-ghast-comparison.md` item 5); loca | PARTIAL | English and Hindi; more languages need strings reviewed by people |
| G-GH-08 | Theme: system, light, dark; greeting line "Start with something that c | BUILD | BUILT | system, light, dark (e2e) |
| G-GH-09 | Auto-update with release notes, notarised installer; companion tray us | BUILD: signed updates with a key pinned in the app, no third-party analytics, update notes | PARTIAL | signed announcements (row 25); installer waits for signing |
| G-GH-10 | Crash diagnostics "only with approval", feedback with consent; but Pos | BUILD a local diagnostics bundle the user can read and attach; no analytics SDKs. Honest c | PARTIAL | kaku diagnostics: readable report of versions, checks, counts and event kinds, no names, paths or text; no crash capture, no screen (diagnostics.md) |
| G-GH-11 | One calm composer for everything: attach (+), model picker, reasoning  | IMPROVE: one composer plus our additions: route badge and what-leaves line | PARTIAL | one composer with route and meter; attachments and picker not in it |
| G-GH-12 | Attachments: PDF, DOCX, ZIP, images | BUILD attach through the offline parser, each file labeled on entry | PARTIAL | attach files in the web chat (Word, Excel, PDF text layer, web pages, text, archives) via an isolated reader; OCR owed: AnythingLLM asPDF read, falls back to OCR, we do not yet (attachments.md) |
| G-GH-13 | Model picker with search, families, capability badges, reasoning level | BUILD with eligibility, not only capability: each model shows route class and whether the  | LATER | one route until NEAR credits |
| G-GH-14 | Permission shield in the composer (screenshot shows a yellow shield; w | KEEP; adopt a visible mode indicator in the composer showing the current policy | PARTIAL | approvals badge in the header of every screen; not inside the composer, no mode choice by design (web-shell.md) |
| G-GH-15 | Context meter, automatic and manual cleanup, custom limit, prompt cach | BUILD; caching only on routes whose docs allow it (NEAR docs warn about cross-customer pre | BUILT | meter, warning, automatic summary (sv49-sv56) |
| G-GH-16 | Usage page: per-model cost, custom date range, response speed | BUILD a local usage ledger per route | PARTIAL | usage ledger per route for chat replies with date range, measured vs estimated tokens and speed (web, service, terminal); no cost (no prices known), ask and reviews not counted (web-shell.md) |
| G-GH-17 | Slash commands and a commands menu for tools, skills, MCP, saved promp | IMPROVE web and desktop to match | PARTIAL | web slash commands menu built (navigation and review commands); tools/skills/MCP/saved-prompt entries and in-conversation commands not built (web-shell.md) |
| G-GH-18 | Plan mode | BUILD: plan is shown before any effect; the plan is not an approval | LATER | plan mode with the worker loop |
| G-GH-19 | Personal AGENTS instructions for chat and coding | BUILD as labeled "owner instruction" memory, kept separate from data memory so injected te | BUILT | owner standing instructions: encrypted store, owner-session only, global + per project, one labelled system block, logged without words; web, terminal; not applied to ask or reviews (owner-instructions.md) |
| G-GH-20 | Interactive previews and artifacts rendered in a sandbox page (CSP `co | ADAPT the idea: render model-made HTML in a sandboxed frame with no network, no storage | EXCLUDED | no model-written HTML is run |
| G-GH-21 | App-context capture with an optional global shortcut | BUILD as an explicit one-shot capture with preview and label, not a background recorder | LATER | explicit one-shot capture |
| G-GH-22 | Voice input (local Whisper) and local TTS | BUILD local only; remote speech route only for Public-label audio | EXCLUDED | voice not built; remote speech excluded |
| G-GH-23 | Chat search (Cmd+K), fork, archive, rename, Recent list | BUILD search over history in the vault, results filtered by label | PARTIAL | search and rename built (web, terminal); fork and archive not built |
| G-GH-24 | Reply buttons, table copy and export, code copy, keyboard access | BUILD; export of a result is a release (goes through the one approval path) | PARTIAL | code copy; no table copy or export of a result |
| G-GH-25 | Vision and images | BUILD later; images get a label at entry (screenshots hold secrets) | LATER | images get a label at entry |
| G-GH-26 | Video generation | EXCLUDE (no private route) | EXCLUDED | no private route |
| G-GH-27 | Web research: automatic search, fallback when blocked, page understand | BUILD through the kernel: every query and fetched page is an egress with a label check; fe | BUILT | search and read with approval, honest failures, pages in chat (rows 2 and 24) |
| G-GH-28 | Scheduler and cron skill; missed runs recorded for replay | ADAPT the store shape (MIT). New rule: an unattended run can never self-approve a release | BUILT | scheduled reviews that never approve, missed-run ledger (row 20) |
| G-GH-29 | Many models through 0G Router, 0G Advanced providers and other vendors | IMPROVE: keep route eligibility the first-class idea; add NEAR when a key exists | LATER | more routes with NEAR |
| G-GH-30 | "TeeML verified" display on supported models; docs admit the on-chain  | IMPROVE: per-reply verified panel with fresh nonce; label what was checked and what not | PARTIAL | model and gateway proofs shown; per-reply signed verification blocked on NEAR key |
| G-GH-31 | Plugin-registered providers (for example a local Ollama plugin) | BUILD: a provider plugin declares a route class; the kernel, not the plugin, decides eligi | LATER | provider plugins |
| G-GH-32 | Coding mode: pick a project, describe the issue, allow edits, review d | IMPROVE: add checkpoint and rollback (matrix E cites cline and opencode snapshots) | BUILT | fixes with checkpoint, apply, undo (ff01-ff14) |
| G-GH-33 | Local coding agents (Codex, Claude Code) join as teammates via ACP ses | ADAPT the session manager shape (MIT). Do NOT copy the approval trigger (section 4, row 3) | LATER | local coding agents as teammates |
| G-GH-34 | Local command execution: workspace or full mode, working-directory che | ADAPT session lifecycle, tiers and orphan recovery (MIT); EXCLUDE the workspace policy | LATER | local command execution |
| G-GH-35 | Policy for "workspace" mode: substring blocklist (`sudo`, `shutdown`,  | EXCLUDE. Not a boundary | EXCLUDED | substring blocklist is not a boundary |
| G-GH-36 | Approval record for ACP and runs: id, tool name, truncated preview, ri | IMPROVE only: adopt expiry that fails closed and the "approval linked to run id and relink | PARTIAL | countdown and fail-closed display built for web and terminal; per-run id relinking and desktop native window deadline not checked (web-shell.md) |
| G-GH-37 | 26 bundled expert agents in five groups (product and engineering, busi | BUILD our own roster; prompts written by us (no copying). Start with roles that fit privat | LATER | expert roster |
| G-GH-38 | Modes: Direct, Roundtable (independent answers, then lead resolves), A | BUILD; Roundtable across routes must respect the label of every participant's route | LATER | modes |
| G-GH-39 | Lead owns the outcome; handoffs, dependencies, completion conditions,  | BUILD; a human checkpoint is a kernel approval, not a model message | LATER | lead and handoffs |
| G-GH-40 | Activity view (running, review, blocked, stalled), interrupted-work re | BUILD on the run ledger (GH-74) | LATER | activity view of running agents |
| G-GH-41 | Reusable "methods" with version history, edit and restore | BUILD: method = content-addressed text plus history; a method that changes is re-approved  | LATER | methods with history |
| G-GH-42 | Group conversations, shared tasks, collaborator file permissions | BUILD last; multi-person needs identity and per-member keys (hosted-mode work) | LATER | group conversations need identity |
| G-GH-43 | Three write paths: explicit in chat, from the Activity Recorder, manua | KEEP our gate; recorder path see GH-53 | KEPT | owner gate |
| G-GH-44 | Recall at conversation start: embed first message, sqlite-vec nearest  | IMPROVE: relevance recall with a recall card that also lists notes WITHHELD and the reason | BUILT | relevance recall with the notes shown (sv43, row 3) |
| G-GH-45 | Local vector store with local embedding model or remote provider; swit | ADAPT RRF and pool sizing (Apache); vectors stored in SQLCipher with the memory's label an | BUILT | vectors and rank fusion, measured (memory.md) |
| G-GH-46 | Consolidator archives overlapping or long-tail memories; "delete" is s | IMPROVE: consolidation only as owner-approved proposals; never keep archived text. Do not  | BUILT | owner-approved merge with undo (me32-me40) |
| G-GH-47 | Memory panel: stats, semantic search, filter, edit (re-embeds), soft d | IMPROVE: stats and search filters | PARTIAL | search, correct, history, merge; no stats |
| G-GH-48 | Import memory from 14 other agent systems (Claude Code, Codex, mem0, L | ADAPT the adapter contract and read-only rule (Apache). Imported text is untrusted-origin  | BUILT | importers for other assistants' notes as untrusted (row 14) |
| G-GH-49 | Serve the aggregated memory back to any agent over MCP and a CLI (14+  | EXCLUDE the serve-back door. Keep only internal use of the search library | EXCLUDED | no serve-back door |
| G-GH-50 | Why a memory was not used: explain-turn with exclusion reasons (dorman | ADAPT the reason list (Apache, small enum) and presets; add `label_gated` and `route_ineli | PARTIAL | withheld and left-out counts shown; no per-note reason list |
| G-GH-51 | Action policies stored as memories (`do_not_push`, `prefer`, `procedur | ADAPT with one rule: a memory-derived policy may only narrow behaviour for the model, neve | LATER | action policies as memories |
| G-GH-52 | Short-term to long-term promotion and memory garbage collection run on | BUILD the idea only as proposals the owner accepts; name known, mechanism NOT VERIFIED | PARTIAL | owner accepts suggestions; no garbage collection |
| G-GH-53 | Activity Recorder: samples front app, window title, tabs, screenshot t | BUILD, opt-in and default OFF, encrypted store, per-app mute list, label Restricted plus u | LATER | opt-in activity recorder |
| G-GH-54 | Learned skills: the agent writes reusable skills from work; a threshol | BUILD as proposals; a skill is instructions, so it is untrusted until the owner approves i | LATER | learned skills as proposals |
| G-GH-55 | 30 bundled skills (tasks, scheduler, notebook, work-log, plan-mode, co | BUILD our own; do not copy text | LATER | bundled skills |
| G-GH-56 | Updated skill content must be re-approved (changelog: "installed plugi | ADAPT: approval pinned to a content hash; any change re-prompts | LATER | skill re-approval by hash |
| G-GH-57 | Partner Mode: scenario packs (work, companion, quiet), initiative engi | ADAPT the structure: schema-constrained personal model, anchored initiative, boundaries en | LATER | partner mode |
| G-GH-58 | Plugin store: 496 plugins in 16 categories, Skills / Manage connection | BUILD our own catalog and format (study-only source). Take the structure: manifest, detail | LATER | plugin store |
| G-GH-59 | Package format: `plugin.json` plus `mcp.json` plus `skills/` plus `hoo | BUILD with a signed catalog (Ed25519, pinned key) and signed packages. Their digest comes  | LATER | package format and signed catalog |
| G-GH-60 | "Installing does not grant access; install, authorisation and executio | ADAPT as policy: install grants nothing; each effect is a kernel grant with its own approv | LATER | install grants nothing |
| G-GH-61 | Plugin lifecycle hooks `SessionStart`, `UserPromptSubmit`, `SubagentSt | EXCLUDE command hooks by default. BUILD declarative hooks that only add context; hook outp | LATER | declarative hooks |
| G-GH-62 | Provenance and licence audit of ported plugins: official source verifi | ADAPT the labels and the "runtime claims never exceed recorded evidence" rule for our cata | LATER | provenance audit |
| G-GH-63 | OAuth connectors: 31 plugins flagged `oauthRequired`; 538 MCP services | BUILD later: tokens live in the vault, never in model context; the audit job idea (automat | LATER | OAuth connectors |
| G-GH-64 | MCP client: stdio, SSE, streamable HTTP; headers and env with `$VAULT: | ADAPT the manager lifecycle (MIT) but run every call through the broker. Secrets by refere | LATER | MCP client behind the broker (trace row 21 owed) |
| G-GH-65 | Tool governance in the companion: none. `call_tool` checks only that t | IMPROVE ours when built: tool annotations (read-only, destructive), pinned hash of tool na | BUILT | tool annotations, tiers and owner leases (agent-leases.md) |
| G-GH-66 | MCP auto-discovery: the companion reads other apps' `mcp.json` (Claude | EXCLUDE silent import. Offer a preview list the owner accepts one by one | EXCLUDED | no silent import of other apps' MCP configs |
| G-GH-67 | Registry: 10 curated servers, fetched live from a Git branch on every  | BUILD a signed, versioned registry with pinned package versions and digests. Keep the nice | LATER | signed registry |
| G-GH-68 | Computer Use on macOS: 11 core primitives (screenshot with accessibili | BUILD later on Cua Driver as already decided (`NEAR_VERSION_OF_GHAST.md` section 12), our  | LATER | computer use |
| G-GH-69 | Five safety layers: OS permission, per-app approval gate (mutating act | ADAPT the layering and fail-closed timeout. Improve: "Allow Always per app" is too broad;  | LATER | safety layers for computer use |
| G-GH-70 | Browser Relay: desktop drives the user's real Chrome over CDP via a lo | BUILD later as a separate, higher tier. Keep our capture-only extension as is. Relay needs | LATER | browser relay |
| G-GH-71 | Assistant extension: side panel, content scripts on 15 DeFi and social | KEEP ours. ADAPT native-messaging pairing (host manifest with `allowed_origins` for one id | KEPT | our extension |
| G-GH-72 | Native app bridges: Calendar, Reminders, Contacts, Notes, Finder, Safa | BUILD each as a kernel effect, default OFF, reads are disclosures; process terminate and s | LATER | native app bridges |
| G-GH-73 | Media: HEIC and HEIF normalisation via `sips`, macOS Vision OCR | ADAPT concept with OS tools (MIT code optional) | LATER | media normalisation |
| G-GH-74 | Run and approval ledgers: run states, session links, approval records, | ADAPT state names and orphan marking (MIT); keep our log as the truth | PARTIAL | job store and signed log; no separate run ledger |
| G-GH-75 | Durable agent turn host: the LLM loop runs in the daemon, streams over | IMPROVE: event stream with cursor so a closed window reattaches; per-turn secrets never wr | PARTIAL | change notices (row 17); no event feed with a cursor |
| G-GH-76 | Self-check and repair: `self-check --json` reports config, token, work | ADAPT the JSON shape (MIT) for our readiness screen: Ollama reachable, model present, sand | BUILT | readiness with one-click repair (row 13) |
| G-GH-77 | Tray "Permissions & Safety" page: two layers (system grant versus comp | ADAPT the two-layer model and "why was this blocked" log | LATER | tray permissions page |
| G-GH-78 | Bridges to Telegram, Discord, Slack, Feishu, WeChat: channel mapped to | KEEP the rule that a chat service never receives content that the label forbids. ADAPT the | KEPT | Telegram carries counts and ids only; other chat services later |
| G-GH-79 | Bridge approvals: triggers on long reply (800 chars), outbound file, t | ADAPT triggers and Edit and Send. Rules: an edited message is re-hashed and re-approved; " | EXCLUDED | approval never happens in chat |
| G-GH-80 | Mobile and remote access: only chat bridges and an extension; no nativ | BUILD the passkey approval path later; Ghast gives no mobile reference | LATER | passkey approval path |
| G-GH-81 | Encrypted sync of chosen categories: key from the wallet private key b | ADAPT envelope-with-AAD, KDF label, and write-order-with-pointer-last (MIT). IMPROVE: rand | PARTIAL | encrypted backup and restore (row 7); no sync |
| G-GH-82 | Honest limits page: "What this stack does not protect against" (malwar | KEEP and extend with a per-surface "what leaves" table | BUILT | LIMITS.md plus the page's per-surface cards and its 'does not protect against' list |
| G-GH-83 | Local data: wallet, tokens and bridge credentials encrypted; conversat | KEEP | KEPT | encrypted at rest |
| G-GH-84 | Data disclosure and privacy pages: "AI processing is a transfer", back | BUILD a plain "what leaves, to whom, when" page generated from the route table | BUILT | same page; the address, not the label, decides what stays here (privacy-page.md) |
| G-GH-85 | Pricing and limits: download free, model usage billed (0G balance), no | EXCLUDE points, referrals, seasons, token (H7, H50 CUT). Pricing for us is open | EXCLUDED | no points, referrals or token |
| G-GH-86 | Capability economy: defined claim, evidence, bond, three-day challenge | Out of Ghast-parity scope; keep as later work (`NEAR_VERSION_OF_GHAST.md` section 4) | LATER | capability economy |
| G-GH-87 | Documentation as tested examples: stable-version walkthroughs with the | ADAPT: every public how-to has a test that executes it (our own rule, section 5.30 of the  | BUILT | `scripts/docs_check.py` runs in scripts/test: every link, path, script, test target, command, flag, the MCP tool table and the config example in the public docs are checked against the repository and the built binary; it found four gaps in docs/mcp.md (fixed) and a mutation run proves it fails on each kind of false claim |
| G-GH-88 | Atomic JSON persistence with a `.bak` copy and 0600 file mode (0700 di | ADAPT for any non-database file (MIT pattern); on Windows use ACLs | BUILT | kk_vault::durable atomic and fsynced writes with a whole backup; vault.key created atomically with a copy and restored; Telegram bound chat and update marker 0600 atomic (durable-files.md) |

## Matrix A: Terminal and sessions (sections A01-A46)

| Key | Capability | Matrix decision | Status | Evidence or reason |
|---|---|---|---|---|
| A-A01 | Persistent sessions: save, list, resume | see the section in matrix A | BUILT | encrypted kept conversations, list, open, forget (c81c227) |
| A-A02 | Session picker | see the section in matrix A | BUILT | /sessions picker (picker.rs) |
| A-A03 | Resume at launch | see the section in matrix A | PARTIAL | kaku --resume opens the newest kept conversation at launch, tested in a real terminal; no chosen-ID resume or picker; reference sources not re-read (terminal-transcript.md) |
| A-A04 | Fork or branch a session | see the section in matrix A | PARTIAL | /branch [N] copies a kept conversation and opens the copy; no version switcher or message picker; terminal references not re-read (terminal-transcript.md) |
| A-A05 | Rewind to an earlier prompt, with file revert | see the section in matrix A | LATER | rewind with file revert; fixes have undo |
| A-A06 | Message queue while busy | see the section in matrix A | BUILT | queued messages while an answer arrives (7d5a796) |
| A-A07 | Steer and interrupt, busy-input modes | see the section in matrix A | BUILT | Esc twice stops an arriving answer (88ce0cc) |
| A-A08 | Interrupt hygiene: never leave the conversation invalid | see the section in matrix A | BUILT | a stopped reply leaves a valid conversation |
| A-A09 | Multiple concurrent jobs and agents: roster, dock, detail, tail | see the section in matrix A | PARTIAL | /status lists reviews; no dock or roster |
| A-A10 | Background work that finishes while you are elsewhere | see the section in matrix A | PARTIAL | live progress of a watched review; no catch-up on return |
| A-A11 | Notifications and terminal status | see the section in matrix A | PARTIAL | bell + generic window title on review finished/stopped, KAKU_NOTIFY=off, focus reporting; pty-verified; no OSC 9/777, no service-error notice (terminal-transcript.md) |
| A-A12 | Permission prompt design | see the section in matrix A | BUILT | approval view with exact bytes and typed digest (11c1446) |
| A-A13 | Remembered permissions and rule languages | see the section in matrix A | EXCLUDED | no remembered permissions for anything that releases data |
| A-A14 | Approval requests under concurrency | see the section in matrix A | BUILT | approval view closes when decided elsewhere (0d4afbc) |
| A-A15 | Slash command registry | see the section in matrix A | BUILT | slash command registry (cmd.rs) |
| A-A16 | Completion popup and argument completion | see the section in matrix A | PARTIAL | command names, folders after /review, and review ids after job commands complete with Tab; no note ids, project names or recipients (terminal-transcript.md) |
| A-A17 | @-mentions and file references | see the section in matrix A | EXCLUDED | no @ references for now: the service must resolve files (matrix A fixed constraint) |
| A-A18 | Path and directory completion | see the section in matrix A | PARTIAL | Tab after /review completes folders from /api/fs (service view, prefix, safe names, encoded, stale-proof); pty-verified; OpenCode directory-completion not read, other commands not covered (terminal-transcript.md) |
| A-A19 | Multi-line composer and editing quality | see the section in matrix A | BUILT | multi-line composer (7d5a796) |
| A-A20 | External editor for drafts | see the section in matrix A | BUILT | Ctrl-G external editor (afb6ec0, c8c148e) |
| A-A21 | Paste handling | see the section in matrix A | BUILT | folded pastes |
| A-A22 | Input history | see the section in matrix A | BUILT | saved command history; history recall never replaces a draft (3f24557) |
| A-A23 | Drafts and stash | see the section in matrix A | BUILT | draft stash (01efa8c) |
| A-A24 | Shell escape and inline command interpolation | see the section in matrix A | EXCLUDED | no shell escape: the terminal has no effects of its own |
| A-A25 | Side question that does not enter the conversation | see the section in matrix A | PARTIAL | /aside: session-less chat, apart card, kept out of history/export/search; service unchanged (sv108); pty-verified; OpenCode/Codex sources read only to matrix depth (terminal-transcript.md) |
| A-A26 | Goal, plan mode, todo panel | see the section in matrix A | LATER | plan and todo with the worker loop |
| A-A27 | Diff viewing and edit flows | see the section in matrix A | PARTIAL | fix diffs coloured in the terminal with markers and weight kept for no-colour; no line numbers, split view or word marks (terminal-transcript.md) |
| A-A28 | Streaming rendering | see the section in matrix A | BUILT | streamed answers (71e541f) |
| A-A29 | Markdown and syntax rendering of model text | see the section in matrix A | PARTIAL | answers shown formatted in the terminal: own reader ported from the web one and checked against it on 42 shared cases; links visible, images never fetched; no tables or code colours; Codex renderer read ~370 of 3,267 lines (terminal-transcript.md) |
| A-A30 | Long output, collapsing, verbosity, virtual lists | see the section in matrix A | PARTIAL | windowed drawing + long cards folded with Ctrl-O toggle; verbosity levels, per-card fold and card memory still owed (terminal-transcript.md) |
| A-A31 | Transcript view, search, copy, export | see the section in matrix A | PARTIAL | terminal /transcript search, /copy (asks, OSC 52) and /export (service writes a new 0600 file in exports); no overlay or region selection (terminal-transcript.md) |
| A-A32 | Scrollback model: alt screen versus inline, mouse | see the section in matrix A | BUILT | alt screen, scroll back, restore on exit (tui_pty.py 27/27) |
| A-A33 | Keyboard model, keymap config, command palette | see the section in matrix A | LATER | keymap config |
| A-A34 | Themes and colour | see the section in matrix A | PARTIAL | a theme exists; no theme choice |
| A-A35 | Accessibility | see the section in matrix A | PARTIAL | kaku tui --linear: append-only plain lines, no control bytes, overlays as prompts; unit + pipe e2e; NOT run with VoiceOver (terminal-transcript.md) |
| A-A36 | Error recovery and process lifecycle | see the section in matrix A | BUILT | terminal restored after errors and exit (tui_pty.py) |
| A-A37 | Reconnect and event replay | see the section in matrix A | PARTIAL | watched reviews are polled every 0.7 s; no event replay (row 17 exclusion) |
| A-A38 | Headless and programmatic modes | see the section in matrix A | PARTIAL | kaku review/jobs/findings/chat with --json and distinct exit codes; cannot approve; no ask, kept-session or follow (terminal-transcript.md) |
| A-A39 | Configuration and diagnostics | see the section in matrix A | PARTIAL | `kaku telegram doctor`; no general `kaku doctor` |
| A-A40 | Status line, title, usage | see the section in matrix A | PARTIAL | status line shows how full the model's window is after each answer; no cost or speed (terminal-transcript.md) |
| A-A41 | Clipboard | see the section in matrix A | PARTIAL | /copy puts the last reply or the whole conversation on the clipboard through OSC 52 after asking; no paste handling (terminal-transcript.md) |
| A-A42 | How the references test a terminal UI | see the section in matrix A | BUILT | real-terminal harness with canary and 27 checks (tui_pty.py) |
| A-A43 | Client/daemon split and shared service | see the section in matrix A | BUILT | terminal is a client of the same service |
| A-A44 | Safe rendering of untrusted text | see the section in matrix A | BUILT | untrusted text is cleaned before drawing (app.rs clean) |
| A-A45 | Context compaction and token limits | see the section in matrix A | BUILT | summaries and meter come from the service (sv49-sv56) |
| A-A46 | Terminal-hosted extension surfaces (plugins, MCP manager, skills, voic | see the section in matrix A | LATER | plugins, MCP manager, voice |
