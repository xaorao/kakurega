# Live research (web pages the owner chooses)

## Target experience
- The owner can add facts from the web to a conversation without any of their own data leaving the computer.
- Before anything is requested, the owner sees the exact addresses. Nothing is requested without a per-request approval that names those addresses.
- Fetched pages are evidence with a source address, a time and a hash, and they are never trusted as instructions.

## References
- Hermes Agent `tools/url_safety.py` (MIT, commit 29a61cb7). READ: lines 1-140 (module contract, address classes, metadata hosts, credential-named parameters). READ since: lines 140-651 (authority ambiguity, fake-IP ranges, connect-time resolution and pinning, redirect target); NOT READ: `web_tools_extract.py`, `web_result_cache.py`, `web_tools_truncate.py`, `web_tools_rescue.py`. Only `web_tools.py` lines 1-120 of the tool module were read.
- Not studied for this slice: Claude Code and Codex web tools, Perplexity-style search, OpenClaw's browser tools. Search providers were not studied; the search design below is independent.

## Decision
- Built independently (REBUILD from behaviour): `kk-fetch` is the only door. Rules: https only, no credentials in the address, port 443 only, no credential-named parameters, no local or internal names, every resolved address must be public (a mixed answer is refused), the connection is pinned to the checked addresses through ureq's `Resolver`, redirects are not followed by the client and are re-validated by us from the start (max 4), body capped at 1 MiB, text types only, text capped at 60,000 characters, scripts, styles, frames and hidden Unicode removed. No cookies, no authorisation headers.
- Service flow: `POST /api/research/propose` returns the exact addresses and a digest and sends nothing; `POST /api/research/{id}/fetch` needs `approve: true` and that digest, works once, and expires after ten minutes. Pages are held in memory for an hour, never written to disk, offered to chat by `sources`, and given to the model as untrusted data; instruction-like pages are withheld. The log keeps counts and host names only.
- Addresses containing secrets (shared detector) are refused.

## Weaknesses of the source addressed
- Hermes validates hostnames and re-applies the policy at connect; ours pins the connection to the validated set so a second DNS answer cannot be used. Mixed public/private answers are refused rather than filtered.

## Limits (stated, not hidden)
- NOT BUILT: a built-in search provider (none is bundled on purpose, see Search), robots.txt handling, PDF and other non-text types, JavaScript-rendered pages, per-site allow lists, a request budget, persistent saved research.
- Pinning relies on `ureq`'s resolver hook; TLS certificate validation uses the library defaults (webpki roots). Not tested against hostile TLS servers.
- Text extraction is a reader, not an HTML parser; unusual markup yields less text.
- `looks_like_instructions` uses the same phrase list as memory; it is a filter, not a guarantee.

## Acceptance
- `cargo test -p kk-fetch` (fe01-fe05; fe06 needs the internet and ran once against example.com), `cargo test -p kk-service --test flow sv26`, browser e2e (propose, refusals, nothing sent, cancel). Mutations of each rule failed the tests.

## Search (added)
- Provider: a SearXNG-style JSON endpoint the owner chooses with `KAKUREGA_SEARCH_URL` (https, port 443, public address, no credentials or query in the address). None is bundled, so no third party receives anything unless the owner picked it.
- Flow: `/api/search/propose` shows the exact words and the one site, sends nothing; `/api/search/{id}/run` needs `approve: true` and the digest of that exact request, works once, expires after ten minutes. Words that look like secrets are refused before anything is sent; the log keeps the provider host and counts, never the words.
- The answer goes through the same hardened fetch path as pages (public-address pinning, redirects re-checked, JSON only, 1 MiB). Hits keep only addresses that could be fetched, deduplicated, at most 8, with cleaned and capped titles and snippets; instruction-like hits lose their text. Hits are suggestions: reading any of them still needs the research approval for those exact addresses.
- Limits: SearXNG response shape only; other providers (Brave, Tavily) would need their own parsers and keys; result quality is the provider's; NOT tested against a live SearXNG instance (no public instance was queried).
- Acceptance: fe07-fe09, sv42 (four guards mutation-checked: digest binding, secret refusal, id-space separation, instruction-like withholding), browser checks against a mocked run.
- Terminal (added): `/search WORDS`, `/read ADDRESS_OR_NUMBER ...`, `/web [clear]`. Each shows what would go out and waits for y; any other key sends nothing. Approved pages go to the model as untrusted `sources` on later messages (five at most); `/clear` and `/web clear` drop them. Acceptance: tu35, tu36 (digest binding, withheld-page and cancel paths mutation-checked). A real-terminal (pty) run of these commands against a live service was not done.
- After the full read of Hermes `url_safety.py`: added trailing-dot host normalisation and the Azure wire server address to the refused set (fe10, both mutations fail). Hermes' fake-IP exemption and its private-URL opt-out are deliberately not adopted: a research fetch from this app has no reason to reach a private network. Its trusted-private-host list is a product-specific exception and is not relevant.
