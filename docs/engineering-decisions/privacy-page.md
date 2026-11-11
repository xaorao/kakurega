# Where your data goes (ledger rows B-W02 and G-GH-84)

## Target experience
- One page that says, in plain words and from the running service, what stays on this computer and what can leave it, right now: the model, notes, reviews, web search, Telegram, the browser extension, AI agents, updates and telemetry, and what the product does not protect against.
- Nothing on it is a brochure claim. Each card changes with the real state, and a claim that cannot be backed says so ("not reported") instead of reassuring.

## Source (read by me)
- Ghast desktop docs (public documentation of a closed product), `docs/product-docs/en/security/overview.md` (64 lines, FULL): a single page of trust boundaries as a static table, an "assumes" list and a "does not protect against" list. `security/local-data-storage.md` (101) and the sibling pages NOT READ. Ideas only; no text or layout copied.
- Weakness in that approach, fixed here: a static page can drift from what the software does. Ours is generated from live facts and checked against them in the browser suite.

## Decisions
- BUILD `GET /api/privacy` (owner session): structured facts, no prose. The page turns them into sentences in English and Hindi.
- IMPROVE over the reference: "stays on this computer" is decided from the model's address, not from the label the route gives itself. A route that calls itself device-only but points at another computer is shown as running elsewhere. Loopback is recognised exactly (`localhost`, any loopback IP); `127.0.0.1.evil.example` and `localhost.evil.example` are not this computer. Only the host is shown: a password, path or query in the address never reaches the page.
- The same fact now drives the header badge and the footer of each chat answer, which used to say "Private" for every real route. With no reported address they claim nothing.
- Counts only: the page shows how many notes there are, never what they say.
- EXCLUDED: a per-request network log (that is the browser's own tool); localisation of the "not protected against" list beyond Hindi.

## Evidence
- Service: sv77 (nine address cases including look-alike hosts, credentials, no endpoint), sv78 (class, test mode, every door closed then opened: search, Telegram file, extension pairing, an owner lease), sv79 (counts without text, owner only), sv80 (the route in `/api/config` and the admin overview carries host and stays_here, never a password), sv81 (a chat answer carries stays_here by address).
- Web: pv01-pv03 for the sentence rule; browser e2e 231/231 including that every card agrees with a fresh read of the API, a mislabelled route is not called local, an address with markup is shown as text and runs nothing, the badge and the answer footer follow the address, axe on each state, the screenshot read.
- Mutations: 11 + 2 on the facts, 5 on the sentence rule, 4 on the route and chat reply, and two browser mutants (badge and footer ignoring the address). All caught; a Telegram file that is valid JSON without a chat number and a really paired extension were first untested and now have cases.

## Limits (written next to the claim)
- The page describes this service's own view. It cannot see what the operating system, the browser, Ollama or another program on this computer does with the data.
- Updates and telemetry are stated, not measured by the service: the browser suite proves only that the web page itself contacts no address but this computer.
- When a second route (NEAR AI Cloud) exists, whether a prompt really goes only to the attested enclave is NOT VERIFIED until credits allow a live run; the page then reports what the route says, with the host.

## What left this computer, as a list that updates (plan item 7)
- Target: one place where the owner sees, as it happens, each thing that left this computer and each thing that was kept from a model.
- Source: the signed log, and nothing else. `GET /api/outbound` reads the last 1,000 entries and turns the ones about leaving or holding back into rows: a question to a model that is not on this computer (host, characters, values sent as placeholders), an approved search, fetched pages, an approved tool call, a delivered release, and parts kept from a model (how many held a secret, how many read like orders, and at which door). Two entries were added to the log for this: `outbound.model`, written where every question to a model elsewhere passes, and `held.back`.
- A question to a model on this computer is not listed, because it does not leave.
- The privacy page shows the list, refreshed every few seconds and on the service's change signal.
- Evidence: kk-service ob01 (which entries become rows; a row holds no more than the log did) and sv254 (a model elsewhere: the row with its host and size, the held-back secret; a model here: no row for the question; no word of the question, the file or the key in the list); `tests/e2e/break_web.mjs` (the page lists what each attempt held back and nothing as having left).
- Limits: the list is as complete as the log entries behind it. Reviews are not in it yet: a review on a NEAR route is recorded in the usage record, not here. The Telegram bridge's messages are not logged as entries yet, so that row never appears.
- Two more cards, found missing while reading the page as a user: "GitHub (read only)" (signed in or not, and that it is contacted only on the owner's request) and "The coding agent on this computer" (installed or not, and that text goes to it only on a hand-over or a yes to a team task, which is not private). Facts come from `/api/privacy` (`github.connected`, `coding_agent.installed`). Found on the way: detecting the coding agent started its program on every page that asked, which made the privacy request slow enough to be cut off by a quick change of screen; the answer is now remembered for a minute. Evidence: `run.mjs` 511; kk-service privacy tests 3.
