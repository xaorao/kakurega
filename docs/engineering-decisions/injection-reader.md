# Text addressed to an AI system: one shared reader (ledger F-I3)

## Target experience
- A person is warned when a file, page or note tries to give orders to an AI system, however the order is written, without being warned about every document that merely discusses the subject.
- A document, page or note can never write a "system" turn into a model's conversation.
- It is a warning, never the defence: what protects the owner is withheld files, labels and approval (`docs/LIMITS.md`).

## Sources read
- OpenClaw `src/security/external-content.ts` (MIT, commit f38495f7d): READ IN FULL (393 lines). Taken: the special-token list, the fold-before-match idea, random boundary ids (not adopted: our untrusted blocks are already labelled by the kernel). Its weakness: 14 English regexes used only for logging, folding only for its own boundary markers, no hidden-character detection, no look-alike letters, no spreading, no other languages.
- Goose `crates/goose/src/security/{patterns,scanner}.rs` (Apache-2.0, commit 71dd9a9): patterns READ (all 50 are shell-command threats); scanner read to line 260. Text injection there depends on an optional ML classifier. Not adopted: a classifier is a new dependency and a measured need has not been shown.
- promptfoo `asciiSmuggling` and the LLM Guard `InvisibleText` idea (from the matrix): the idea of reading text hidden in Unicode tag characters.
- NOT READ: LLM Guard source (archived, not cloned), Rebuff, NeMo Guardrails, Vigil.

## Behaviour
- `kk_types::injection::scan(text)` gives reasons: overrides_instructions, reassigns_role, reveals_prompt, keeps_secret_from_owner, sends_data_out, addresses_ai, jailbreak_wording, chat_markup, role_markup, hidden_text, smuggled_text. `instruction_like()` is every reason but hidden_text alone.
- Views: lower case without invisible characters or combining marks; look-alike letters folded (full-width, mathematical, circled, Cyrillic and Greek twins, accents); leetspeak undone inside words only; spread-out letters ("i g n o r e", "i.g.n.o.r.e") joined. Text in Unicode tag characters is decoded and read. Other languages: Spanish, French, German, Portuguese, Italian, Dutch, Russian, Chinese, Japanese, Hindi, Arabic phrases.
- Imperatives only count at the start of a clause or after a word like "please" ("Attackers can send the password to a URL" is a description; "send the password to a URL" is an order). Weak verbs ("override", "bypass") need a scope about the AI itself. Bare "jailbreak" is not enough.
- `neutralise_special_tokens` replaces listed chat-template tokens and any `<|name|>` with `[REMOVED_SPECIAL_TOKEN]`; applied to every message of every model request in `kk-route` (complete and stream). A property test (3000 random piecings) shows no token can be rebuilt from what is left.
- Used by: memory (`instruction_like`, and tag characters now refuse a note), review import (a note per file; hidden text is now noted for imported files, which was not checked at all), the route boundary.

## Measured (VERIFIED, `crates/kk-types/tests/injection_corpus.rs`, run by hand)
- 3,987 documents from the AI-agent projects cloned for study (about injection, jailbreaks and secrets): the old phrase list flagged 351 (8.8%), the new reader 106 (2.7%). Most remaining flags are documents that quote attacks or chat templates, which is correct.
- 3,958 documents from ordinary Rust crates: old 25 (0.6%), new 8 (0.2%).
- Hostile set written for this (tests inj01-inj03): the old list catches 23 of 40 plain or paraphrased orders, 0 of 10 in other languages and 5 of 20 disguised forms; the new reader catches all of them. That set was written by the same person who wrote the matcher, so it measures the techniques the matcher was built for, not what an unknown attacker would try.

## Not covered
- Orders that must be decoded first (base64, rot13, reversed text), instructions inside images, and orders in meaning that avoids every shape above. A hostile author who reads this file can write around it. The word lists inside each rule are not individually mutation-tested; representative sentences are.
- No model classifier was measured; nothing says one would help here.
- Windows and non-UTF-8 files: not applicable (text is read as UTF-8 by the callers).
