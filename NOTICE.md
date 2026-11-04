# Third-party notices

Ideas credited (no code copied): Treeship (Apache-2.0), whose JCS module's rule of refusing floats and unsafe integers is the basis of our canonical encoding profile.

Depended on (not copied): nearai-inference-sdk 0.2.1 (MIT) for NEAR AI Cloud attestation verification.

No third-party source files are copied into this repository yet. Each component adopted from another project is listed here with its project, licence and what was taken, and its licence text is kept in `third_party/`.

Secret detection (`crates/kk-secrets`): provider token layouts (prefix, alphabet, length) follow public formats also documented by the gitleaks rule set (MIT, https://github.com/gitleaks/gitleaks); no gitleaks source or rule file is copied. False-positive filtering ideas (templated and indirect values, entropy, sequential strings) follow the behaviour of detect-secrets (Apache-2.0); no code is copied. Prompt-injection and egress-variant ideas were studied in promptfoo (MIT) and AgentDojo (MIT); no code is copied.
