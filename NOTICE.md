# Third-party notices

Ideas credited (no code copied): Treeship (Apache-2.0), whose JCS module's rule of refusing floats and unsafe integers is the basis of our canonical encoding profile.

Depended on (not copied): nearai-inference-sdk 0.2.1 (MIT) for NEAR AI Cloud attestation verification.

No third-party source files are copied into this repository yet. Each component adopted from another project is listed here with its project, licence and what was taken, and its licence text is kept in `third_party/`.

Secret detection (`crates/kk-secrets`): about 190 provider token layouts (prefix, alphabet, length) follow public formats also documented by the gitleaks rule set (MIT, https://github.com/gitleaks/gitleaks, studied at commit 5f6ec56); the ways a value is written around a key (INI, JSON, XML, YAML, several languages, Makefile) and the decoding of percent and unicode escapes follow behaviour read in its generator and `codec` package. No gitleaks source or rule file is copied; the benchmark samples it generates are used locally and are not stored here. False-positive filtering ideas (templated and indirect values, entropy, sequential alphabets) and the detection of a credential written into a comparison and of more password names follow the behaviour of detect-secrets (Apache-2.0, studied at commit 5e14193); no code is copied. Prompt-injection and egress-variant ideas were studied in promptfoo (MIT) and AgentDojo (MIT); no code is copied.

Web fetching (`crates/kk-fetch`): the address classes that are always refused (loopback, private, link-local, carrier-grade NAT, cloud metadata addresses and their IPv4-mapped, NAT64, translated and 6to4 spellings), the rule that every redirect target is validated again, and the refusal of credential-named query parameters follow the behaviour documented in Hermes Agent's `tools/url_safety.py` (MIT, https://github.com/NousResearch/hermes-agent). No source is copied; the implementation is written for this project.

Memory recall (`crates/kk-memory/src/fuse.rs`): the reciprocal rank fusion rule (sum of 1 / (60 + rank)) and the candidate-pool sizing (eight times the limit, between 64 and 512) follow the algorithm in Anamnesis's `crates/search/src/hybrid.rs` (Apache-2.0, https://github.com/Trapezohe/Anamnesis, copyright Trapezohe). The code is written for this project and no file is a modified copy, so no per-file change notice applies; the Anamnesis licence and NOTICE are kept in `third_party/` when any of its files are ever copied.

Backup (`crates/kk-backup`): the shape of the sealed envelope (versioned header naming its algorithm, random nonce, context bound as associated data) follows the structure in Ghast's `companion_service/crates/companion-checkpoint/src/sync.rs` (MIT, copyright 2025 Trapezohe). No source is copied. Depends on chacha20poly1305, hkdf and sha2 (RustCrypto, MIT or Apache-2.0).

Conversation summaries (`crates/kk-service/src/compaction.rs`): the idea of summarising the older part of a conversation under fixed headings while keeping the newest turns word for word, starting at one of the owner's turns, follows OpenCode's `packages/core/src/session/compaction.ts` (MIT, https://github.com/sst/opencode). No source or prompt text is copied; the wording and the safety checks are written for this project.
