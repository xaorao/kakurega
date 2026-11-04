# Secret detection (shared crate `kk-secrets`)

## Why
Matrix F (`capability-matrix/F-desktop-docs-security-tooling.md`) measured Kakurega's own detection: three lists that disagreed (14 prefixes in memory, 8 markers in the importer, 5 in the release check). The importer, which decides what an agent may read, caught 20 of 45 secret-shaped files and withheld about 1.2 percent of ordinary source files for looking like secrets (`inputTokens += x`). The release check only compared exact tokens of 12 characters or more, so a short password, a fragment of a URL, or a base64, hex, URL-encoded or reversed copy of a withheld value passed.

## What changed
- One crate, `kk-secrets` (layer 1), used by the importer, memory and the release check.
- Provider formats (GitHub, AWS, Stripe, Slack, Google, Anthropic, OpenAI, GitLab, npm, PyPI, Hugging Face, DigitalOcean, Shopify, Twilio, Mailgun, SendGrid), private key blocks, signed tokens, URL passwords, NEAR and Solana keys, 64-hex keys after key-like names, recovery phrases. Provider layouts (prefix, alphabet, length) follow public token formats that the MIT-licensed gitleaks rule set also documents; the matcher, filters and wallet rules are our own code.
- Keyword assignments with false-positive filters: quote-aware values; unquoted code syntax, member paths, optional chaining, type annotations, paths, CamelCase and ENV_VAR names, test and placeholder markers, templated values, UUIDs and hash digests are not credentials; a credential needs digits or very high entropy; a long lowercase passphrase after a password keyword still counts.
- Decoded segments: base64 and hex segments are decoded with bounded depth (2), bytes (256 KiB) and segment count, and rescanned (Docker auth blobs, Basic headers, nested encodings). A decode bomb and 2 MB of noise finish inside a fixed budget (test sk03).
- Release egress: for each withheld file, whole tokens of 12+ characters, the values of assignments from 6 characters, and for values of 8+ characters their base64 (three spellings), hex, percent-encoded and reversed forms, plus the password and `user:password` part of credential URLs. Variable names are not values, so naming `PAYMENTS_API_KEY` is allowed. The shared detector also runs over every released file.

## Evidence
`cargo test -p kk-secrets` (corpus: 29 positives and 23 benign look-alikes), `cargo test -p kk-review --test release` rs08 (eight transformed or partial leaks rejected, a variable name accepted) and rs09 (provider secrets the old five markers missed). Measured false-positive rate of the generic scanner on real code trees from the local reference clones (`cargo run --release -p kk-secrets --example scan_tree DIR`): opencode/packages 8 of 8,359 text files (0.10 percent), openclaw/src 203 of 28,971 (0.7 percent), hermes-agent 246 of 22,069 (1.1 percent); the remaining hits were inspected and are almost all test fixtures and documentation holding example credentials, which are fail-closed (withheld) rather than leaked. The old importer withheld about 1.2 percent of opencode's real source for non-secrets.

## Limits (not verified or not done)
- Recall was not compared against gitleaks on the same corpus here; matrix F measured gitleaks at 36 of 45 on its own corpus. Our corpus is smaller and written by us; it is not an independent benchmark.
- The recovery-phrase rule has no BIP-39 word list: it uses word shape, repeated words and common function words. Owed: embed the list.
- No PII detection yet (cards, IBAN, SSN, email, phone): matrix F rows P1 to P4.
- Base64 or hex fragments split across lines, other encodings (rot13, unicode escapes), and information leaked by paraphrase are not caught.
- `.kakuignore` (owner-declared withhold list) is not built (matrix F row S8).
