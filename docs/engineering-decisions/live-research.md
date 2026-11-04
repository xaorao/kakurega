# Live research (web pages the owner chooses)

## Target experience
- The owner can add facts from the web to a conversation without any of their own data leaving the computer.
- Before anything is requested, the owner sees the exact addresses. Nothing is requested without a per-request approval that names those addresses.
- Fetched pages are evidence with a source address, a time and a hash, and they are never trusted as instructions.

## References
- Hermes Agent `tools/url_safety.py` (MIT, commit 29a61cb7). READ: lines 1-140 (module contract, address classes, metadata hosts, credential-named parameters). NOT READ: lines 140-650 (DNS pinning transport, fake-IP handling, redirect hook details), `web_tools_extract.py`, `web_result_cache.py`, `web_tools_truncate.py`, `web_tools_rescue.py`. Only `web_tools.py` lines 1-120 of the tool module were read.
- Not studied for this slice: Claude Code and Codex web tools, Perplexity-style search, OpenClaw's browser tools. A search provider (which would send the question text to a third party) is not built.

## Decision
- Built independently (REBUILD from behaviour): `kk-fetch` is the only door. Rules: https only, no credentials in the address, port 443 only, no credential-named parameters, no local or internal names, every resolved address must be public (a mixed answer is refused), the connection is pinned to the checked addresses through ureq's `Resolver`, redirects are not followed by the client and are re-validated by us from the start (max 4), body capped at 1 MiB, text types only, text capped at 60,000 characters, scripts, styles, frames and hidden Unicode removed. No cookies, no authorisation headers.
- Service flow: `POST /api/research/propose` returns the exact addresses and a digest and sends nothing; `POST /api/research/{id}/fetch` needs `approve: true` and that digest, works once, and expires after ten minutes. Pages are held in memory for an hour, never written to disk, offered to chat by `sources`, and given to the model as untrusted data; instruction-like pages are withheld. The log keeps counts and host names only.
- Addresses containing secrets (shared detector) are refused.

## Weaknesses of the source addressed
- Hermes validates hostnames and re-applies the policy at connect; ours pins the connection to the validated set so a second DNS answer cannot be used. Mixed public/private answers are refused rather than filtered.

## Limits (stated, not hidden)
- NOT BUILT: search (needs a provider; the question would leave the computer, so it needs its own approval design), robots.txt handling, PDF and other non-text types, JavaScript-rendered pages, per-site allow lists, a request budget, persistent saved research.
- Pinning relies on `ureq`'s resolver hook; TLS certificate validation uses the library defaults (webpki roots). Not tested against hostile TLS servers.
- Text extraction is a reader, not an HTML parser; unusual markup yields less text.
- `looks_like_instructions` uses the same phrase list as memory; it is a filter, not a guarantee.

## Acceptance
- `cargo test -p kk-fetch` (fe01-fe05; fe06 needs the internet and ran once against example.com), `cargo test -p kk-service --test flow sv26`, browser e2e (propose, refusals, nothing sent, cancel). Mutations of each rule failed the tests.
