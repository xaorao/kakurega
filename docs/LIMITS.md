# What is and is not claimed

This file is the plain list. Evidence for each line is in `TESTING.md`; the status of every planned capability is in `program/parity_registry.json` (most are not built).

## Not verified, and why
- NEAR AI Cloud completions, encrypted inference, signed responses and image provenance: the NEAR API key and credits do not exist yet. Only model attestation was verified, against real public evidence.
- Quality of any real model's review or chat. Local small models find few defects; no bake-off was run.
- A person other than the builder using the product unaided. No first-time-user test has been observed.
- Real Touch ID. The desktop approval flow is tested with the operating-system prompt stubbed.
- Real Telegram delivery. The bridge is tested against a fake Telegram server.

## Known weaknesses
- macOS runs the sandbox inside a Linux virtual machine; that is a development arrangement, not a hardened macOS sandbox.
- The prompt-injection defence for notes is a phrase list plus labelling. Rewording can get past the phrase list; the chat has no tools, which limits the damage.
- Cumulative disclosure checks file coverage and release rate only. It does not detect excerpts that combine into something no single file shows, and says so on screen.
- The event log lives on the same machine as its key. Entries removed from the end are only detectable with an anchor kept elsewhere.
- Hindi text was written by the builder and not reviewed by a native reader. Text that comes from the server stays English.
- A secret detector is heuristic: it has no seed-phrase detection.
- The Electron desktop app is large (about 250 MB, about 450 MB memory); Tauri was not measured.

## Deliberately not built
- Approving a release through a chat service: the chat service would receive the exact bytes. Phone approval needs a native passkey app.
- An MCP tool that returns source, findings, memory or chat text: an MCP client is treated as an untrusted sink.
- Hosted mode, Windows, Linux desktop packaging, an SDK package, collaboration, a plugin store, voice, computer use, on-chain features.

## Process limits
- The builder reviewed their own work; independent review was not done. Mutation checks were used where possible and found real test gaps several times.
- Classifications and trust reasons in the parity registry are the builder's judgement.
