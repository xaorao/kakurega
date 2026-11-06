# Readable diagnostics bundle (ledger G-GH-10)

## Target experience
When something goes wrong, the owner can produce a short report, read all of it in plain words, and decide whether to attach it to a bug report. Nothing is sent anywhere by the program, and there is no analytics.

## Reference
Ghast (closed desktop app, public docs only): "crash diagnostics only with approval, feedback with consent". No source exists to read; behaviour is taken from the matrix row and our own rules. The design is ours: a report the owner reads first, never an upload.

## What was built
`kaku diagnostics [--out FILE]` asks the running service (`GET /admin/diagnostics`, admin token) and prints, or writes with mode 0600 through the durable writer, a text report: version, operating system and architecture, test mode, the model route (id, class, model, whether it runs here), every machine check with its result (a check's own text has the data folder and the home folder shortened to `<data>` and `~`, and is cut at 300 characters), counts (reviews, kept conversations, signed log entries) and the kinds of the last 200 signed events with counts. It states at the end what it never holds.

## Evidence
kk-service sv104 (admin token required; versions, counts and event kinds present; the check text is shortened and bounded; no conversation title, message word, conversation id, data path, home path or token anywhere in the JSON), kk-cli dg01 (plain-words rendering, failures marked, a damaged answer still renders). 5 mutants on the scrubbing and shape caught.

## Not covered
- Crash capture: nothing records a crash; the report describes a running service.
- A screen for it in the web page or the terminal; a check's own text from outside the product is shortened for two known paths only, so a check that prints another path would show it (the owner reads the report first, which is the control).
- Running the command against a real service in the end-to-end suite.
