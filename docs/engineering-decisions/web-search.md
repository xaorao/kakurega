# Web search

## Target experience
- The owner can ask the assistant to look something up, and sees exactly which words go to exactly which site before anything is sent.
- Results are only suggestions of where to look; nothing is read until the owner approves those addresses.
- When a search fails or finds nothing, the owner is told why in plain words and what to do about it.

## References read (by me)
- **OpenCode** (`anomalyco/opencode`, commit fe04656, MIT): `packages/core/src/websearch.ts` in full (234 lines): provider registry, a selection of one provider / random / off, a per-session provider affinity, a cooldown of a provider after HTTP 429 using `Retry-After` (seconds or a date, default 60 s), and failover to another provider. NOT READ: the provider implementations and the schema package.
- **Cline SDK** (`cline/cline`, commit e68a6dd, Apache-2.0): `sdk/examples/plugins/web-search.ts` in full (324 lines): one Exa-backed tool with query, result limit (1 to 10, default 5), domain filter, freshness window in days, country, snippet cut to 800 characters, 30 s timeout, one retry, error text taken from the provider's JSON. NOT READ: the rest of the SDK's tool runtime.
- **LibreChat** (`danny-avila/LibreChat`, commit f4154b6, MIT): located its web-search configuration (`packages/api/src/web/web.ts`, 903 lines, providers SearXNG / Serper / scraper / reranker selection) by search only. NOT READ in depth; the actual search calls live in an external package. No decision rests on it.
- **SearXNG** (`searxng/searxng`, commit 3e95f6c, AGPL-3.0, study and run only, no code taken): ran it in the Linux VM, and read `searx/webapp.py` lines 640-700 (the format gate: a format not listed under `search.formats` gives HTTP 403) and `searx/webadapter.py` (`time_range` is day, week, month or year, anything else is a 400; `pageno`, `language`, `safesearch`).
- Real answers captured from the running SearXNG: four queries (two with 26 results, one infobox-only, one empty) and the failure case (`unresponsive_engines: [["duckduckgo", "CAPTCHA"]]` next to good results). Also what real public SearXNG sites answer to a JSON request through our own fetch path: HTTP 429, plain HTML, and a bot "security check" page.

## What the references and the live run showed
- VERIFIED by running it: a stock SearXNG refuses `format=json` with 403; a real answer has 26 results with tracking-style duplicates, can be only an infobox with no results at all, and carries a list of engines that failed even when results are good.
- VERIFIED through our own fetch path: public sites answer 429 or HTML, so "nothing came back" is the normal case, not the rare one, and the old message (`Status(429)` as raw debug text) told the owner nothing.
- On the sample real answers the first eight results were five from one site (tokio.rs); an owner asking "tokio" saw one site five times.
- OpenCode shows a mature provider layer (failover, cooldown). Cline shows the parameters people actually use: result limit, freshness, domains.

## Ours compared
- Stronger than all three on privacy: no bundled provider, the exact words and the one site approved per search, a digest binds approval to that exact request, words that look like secrets are refused before sending, results are untrusted, instruction-like results lose their text, and the log never holds the words.
- Weaker before this work: failures were shown as debug text, empty answers said nothing, an infobox answer was dropped, one site could fill the list, and there was no freshness window.

## Decisions
| Capability | Decision | Where |
|---|---|---|
| Approval of exact words and site, digest binding, secret refusal, untrusted and withheld results | KEEP (ours is stricter than every reference) | `/api/search/*` |
| Plain-words reasons for 403, 429, 5xx, not-JSON, blocked, unresolved, redirects | BUILD; from the live failures, beats the references' raw statuses | `kk_fetch::explain_search_failure` |
| Failed engines reported; infobox answers kept and listed first; reason when nothing is returned | BUILD from the real SearXNG answers | `kk_fetch::parse_answer`, `/api/search/{id}/run` |
| At most 3 results per site in the 8 | BUILD (a diversity rule; LibreChat uses a paid reranker instead, which would send results to a third party) | `MAX_PER_HOST` |
| Freshness window day, week, month, year, part of the digest the owner approves | ADAPT the idea from Cline's `recencyDays`, mapped to SearXNG's `time_range` | `search_url_with`, web select, `/search --week WORDS` |
| Several providers with failover and cooldown (OpenCode) | EXCLUDE: failover would send the owner's words to a second site the owner did not approve; one approved site is the product's promise | none |
| Domain filter, country (Cline/Exa) | NOT BUILT: the owner can type `site:example.com` in the words, which many engines honour; a country filter is an Exa feature with no SearXNG parameter we verified | owed |
| Result limit as a parameter | NOT BUILT: fixed at 8; the UI would need a control for little gain | owed, low |
| Retry after a failure (Cline: one retry) | EXCLUDE: a retry would be a second send the owner did not approve | none |

## Limits (stated)
- NOT VERIFIED against a live SearXNG through our own fetch path: our guard accepts only public https addresses, so the local VM instance cannot be reached by it. The parser was checked on four real captured answers, and the failure messages through the real fetch path against three real public sites (429, HTML, HTML). A real successful answer from a public JSON-enabled site was not obtained.
- No search site has been chosen (owner decision); public SearXNG sites mostly rate-limit or switch JSON off, so a self-hosted instance with json listed under `search.formats` is the dependable choice.
- The freshness window is only as good as the engines behind the site; SearXNG passes it on where an engine supports it.
- The per-site cap compares host names only; two names for one site count separately.
- Hindi strings for the new controls are unreviewed by a native reader (already an owner item).

## Acceptance
`cargo test -p kk-fetch` (fe20 real-shaped answer, fe21 messages, fe22 windows), `cargo test -p kk-service --test flow search` (sv42 extended: 503, 403, empty with a failed engine, window digest), `cargo test -p kk-tui` (tu35b empty answer, tu35c flag), `cargo test -p kk-cli --test search_windows`, browser checks for the window select and the empty answer. Mutations: 14 on the answer and message rules, 10 on the window (one fmt-reflowed pattern redone), 3 on the terminal's empty state, all caught.
