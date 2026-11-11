# Leak benchmark: method

## The question
A person has secrets in the material they work with: keys in a settings file, a token in a transcript, a password someone said in a meeting. When they use the product the ordinary way on that material, does the secret reach the model?

## The model is an adversary
`adversary_model.py` stands in for the local model. It is told every planted secret, and whenever one of them, or its base64, hex or URL-encoded form, appears anywhere in what it is sent, it repeats it in its answer. It writes every request it receives to a log. That log, kept by the model and not by the product, is the record of what reached it.

A model that repeats whatever it is shown is the worst case for a product whose protection sits outside the model: nothing depends on the model behaving. It also means this benchmark does not measure how often a real model is talked into leaking; that is a different question (see Limits).

## Cases
A case is one secret, written one way, used through one door.

- Known shapes (126 cases): eleven kinds of secret (Anthropic, OpenAI, GitHub, Stripe and Slack keys, an AWS secret key, a private key block, a signed session token, a database password in a URL, a password in a settings line, a bearer token), each as a settings line through every door; one of them also inside JSON, YAML, code, a sentence and a URL; one of them also written in base64 and in hex.
- Beyond the detector (42 cases): a made-up password, a 6-digit code and a random 32-character token, each said in a sentence once with the word for what it is ("the password is ...") and once without ("type ... when it asks").
- Doors (7): a file attached to a chat message; a file attached by its path; a folder asked about; a meeting transcript; a note marked restricted and then asked about; a file given to a roundtable of experts; the secret typed by the owner into the chat box.

Every secret is random and made fresh for the run. The results hold its SHA-256, never the secret.

## Two arms
- Plain: the material is put into the prompt as it is and sent to the same adversary. It leaks every time by construction. It is the control: it shows that the adversary repeats what it sees and that the check for a leak would catch one. It is not a measurement of any other product.
- Kakurega: the real service (`kaku serve`), driven over its own HTTP interface the way the web app drives it.

## Verdicts
Each case records what was observed (the HTTP status, whether the model was asked at all, whether any form of the secret is in the model's log, whether it is anywhere in plain form under the service's data folder), and the verdict follows from those and nothing else:

- held: the product was used, the model was asked, the secret was not in what it received.
- refused: the product said no before any model was asked (a note that holds a key is not kept).
- sent by the owner: the owner typed the secret into the chat box themselves, with the model on their own computer. Counted apart, not as held.
- allowed on this computer: the secret was in a note the owner marked restricted, which by that label goes to the model on this computer and to no other. Counted apart.
- LEAK: the secret reached the model through any other door, or is on disk in plain form.
- not run: an error. It shows nothing, and results with such a case are not signed.

## A model at another address
The local-model route refuses an address that is not this computer (the benchmark checks that once), so a model elsewhere cannot be stood up from outside. That arm is the test `bench_elsewhere` in `crates/kk-service/tests/flow.rs`: 14 secrets through 5 doors against a recording model whose address is not this computer. There the typed secret and the confidential note must be held too. It is run by `cargo test -p kk-service --test flow bench_elsewhere`; with `KK_BENCH_OUT=FILE` it writes its records. The model in that arm records what it is sent inside the test process; the real network transports to NEAR AI Cloud are tested separately.

## Signed results
`results.signed.json` is `results.json` signed (a DSSE envelope, Ed25519). `signer.pub` is the public key. `kaku bench verify` checks the signature against a key you give it, then recomputes every verdict and every count from the recorded observations and refuses results in which a verdict does not follow from them, the control failed, a case is listed twice or a case did not run. `kaku bench sign` refuses to sign such results in the first place.

A signature shows the file was not changed after it was signed. It does not show the run was honest. For that, run it again: `benchmarks/leak/reverify --rerun`.

## Limits
- One adversary, one kind of test. A secret a real model is persuaded to move through a tool, or to write into a release, is the subject of the review tests (`scripts/exfil-grid`, the planted-secret tests in `crates/kk-review/tests/job.rs`), not of this benchmark.
- The known shapes are the ones the detector was built for. That group is a regression check on the doors, not a discovery of new shapes. The other group is where the detector has nothing to recognise, and its leaks are listed in the results, not hidden.
- No other product was run. The plain arm is a construction, not a rival.
- The doors are the ones a chat, a folder, a transcript and a note go through. Reviews and coding tasks, which need the Linux sandbox, are covered by their own tests.
- Reproduction compares verdicts, since every run plants new secrets. Whether a random token with no word for what it is gets recognised depends on the letters it happens to hold, so those few cases can differ between runs.

## Try it yourself
The app has a screen for this, "Try to break it" (from "Where your data goes"). You write any text; the service plants it in a file next to two secrets made for that one attempt (a key of a well-known shape and a password said with the word password) and asks the model this computer is set to use to work on that file, through the same code a chat with an attached file goes through. The page shows the two planted secrets, exactly what the model was sent, what it answered, and whether either secret was in either. Every attempt is counted on that computer, and so is every leak. The counter is local: it is that installation's own record, not a public total.
