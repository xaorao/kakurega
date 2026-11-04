# Policy engine (contract K2, tests G, new G18-G20)

## Target experience
Every action a worker asks for is decided by one readable policy that a person can audit, and a mistake in the policy or in a request makes the answer "no", never "yes".

## References
Cedar 4.14.1 (Apache-2.0, crate `cedar-policy`, pinned exactly); earlier experiment E17 (`private-ai/specs/experiments/cedar/E17_RESULT.md`): Cedar skips a policy that errors during evaluation, so a security `forbid` that errors silently stops applying and an allow can survive. IronClaw uses grant matching rather than a policy language (see `ironclaw-kernel.md`).

## Approach (`kk-policy`)
- The schema (`policy/schema.cedarschema`) and the default policy (`policy/default.cedar`) are plain files in the crate. The engine refuses a policy that does not validate under strict mode against the schema (unknown attributes, wrong types).
- Requests are built from typed facts, never by string interpolation: identifiers go in through the entity API, so a hostile file name or worker id cannot become policy text. Requests and entities are validated against the schema.
- Wrapper rule from E17: any evaluation error reported in the response diagnostics turns the decision into a deny (`EvaluationError`), whatever else matched. Decisions carry the matching policy ids (or the forbid ids) for the evidence log.
- Default policy: read needs level and every compartment covered and known sensitivity; write for agent and parser workers; a model route is permitted only when it is `device_only` or a verified TEE route, and is forbidden for tainted data on anything else, including an unverified TEE route; a tool needs a grant (it must be in the worker's allowed set), read-only tools are allowed, effect tools need a human decision, and untrusted content in the step forbids effect tools unless a human approved; only agents may propose a release.
- The label fields come from the kernel's taint computation (`kk-kernel`), never from the worker.

## Results (VERIFIED_BY_RUN, `cargo test -p kk-policy`)
8 tests: reading rules, write and propose by worker kind, routes by taint with no fallback and public routes never permitted, tool grants and the human-approval rule, empty policy denies all, evaluation error is a deny while a permit matches, ill-fitting policies refused at load (unknown attribute, syntax, schema), identifiers with quotes, backslashes and bidirectional marks are data. Mutation check: 7 of 7 applicable mutations caught (error check removed, validation disabled, level comparison reversed, compartment check removed, sensitivity check removed, verified-route condition removed after a test gap was found and fixed, effective-level mapping).

## Not covered
Policy versioning and signing in the evidence log (policy version hash is not yet recorded per decision); policy authoring UI; per-project overrides; performance under load; Cedar's own bugs (upstream); the mapping of broker ops to capability envelopes (next, with the broker).
