# Third-party notices

Ideas credited (no code copied): Treeship (Apache-2.0), whose JCS module's rule of refusing floats and unsafe integers is the basis of our canonical encoding profile.

Depended on (not copied): nearai-inference-sdk 0.2.1 (MIT) for NEAR AI Cloud attestation verification.

No third-party source files are copied into this repository yet. Each component adopted from another project is listed here with its project, licence and what was taken, and its licence text is kept in `third_party/`.

Secret detection (`crates/kk-secrets`): provider token layouts (prefix, alphabet, length) follow public formats also documented by the gitleaks rule set (MIT, https://github.com/gitleaks/gitleaks); no gitleaks source or rule file is copied. False-positive filtering ideas (templated and indirect values, entropy, sequential strings) follow the behaviour of detect-secrets (Apache-2.0); no code is copied. Prompt-injection and egress-variant ideas were studied in promptfoo (MIT) and AgentDojo (MIT); no code is copied.

Web fetching (`crates/kk-fetch`): the address classes that are always refused (loopback, private, link-local, carrier-grade NAT, cloud metadata addresses and their IPv4-mapped, NAT64, translated and 6to4 spellings), the rule that every redirect target is validated again, and the refusal of credential-named query parameters follow the behaviour documented in Hermes Agent's `tools/url_safety.py` (MIT, https://github.com/NousResearch/hermes-agent). No source is copied; the implementation is written for this project.

Memory recall (`crates/kk-memory/src/fuse.rs`): the reciprocal rank fusion rule (sum of 1 / (60 + rank)) and the candidate-pool sizing (eight times the limit, between 64 and 512) follow the algorithm in Anamnesis's `crates/search/src/hybrid.rs` (Apache-2.0, https://github.com/Trapezohe/Anamnesis, copyright Trapezohe). The code is written for this project and no file is a modified copy, so no per-file change notice applies; the Anamnesis licence and NOTICE are kept in `third_party/` when any of its files are ever copied.

Backup (`crates/kk-backup`): the shape of the sealed envelope (versioned header naming its algorithm, random nonce, context bound as associated data) follows the structure in Ghast's `companion_service/crates/companion-checkpoint/src/sync.rs` (MIT, copyright 2025 Trapezohe). No source is copied. Depends on chacha20poly1305, hkdf and sha2 (RustCrypto, MIT or Apache-2.0).
