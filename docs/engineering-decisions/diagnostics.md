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
- A crash dump, a backtrace, the panic message and any memory are not captured (a message can quote a file, a note or a key). Only the place in the code is.
- A screen for it in the web page or the terminal; a check's own text from outside the product is shortened for two known paths only, so a check that prints another path would show it (the owner reads the report first, which is the control).
- Running the command against a real service in the end-to-end suite.

## Crash reports (ledger G-GH-10, second part)
- Target: when the program crashes, the owner finds a short report they can read in plain words and decide to attach to a bug report. Nothing is sent, nothing is uploaded, nothing is recorded about what they were doing.
- Reference: Ghast's "crash diagnostics only with approval" is closed (public docs only); the design is ours. Sentry-style SDKs were not read and are excluded by principle: they upload.
- Built (`kk-service::crash`): `kaku serve` installs a panic hook that writes `<data>/crashes/crash-<time>-<n>.txt` (mode 0600, the usual message still goes to the terminal) holding exactly: the version, the time, the thread's name and the place in the code, with every personal part of the path removed (a path inside the project starts at `crates/`, one in a dependency at its name and version, anything else is a bare file name). Never the panic message. At most ten are kept. `kaku diagnostics` lists them in plain words (`/admin/diagnostics` gains a `crashes` list). A file in that folder is shown only if every line is exactly what the program writes (fixed header, a path that is relative, no `..`, bounded fields, no extra line, not a link, under 2 KiB); files that are not reports do not use up the places of real ones.
- Found by my own hostile test: the reader first accepted a place like `/Users/alice/x.rs:3` because `/` and `.` are letters it allows; a place must now be relative, never climbing, and contain the line number.
- Evidence: kk-service cr01-cr05 (paths from a Mac, a Windows machine, a dependency, the standard library and nonsense lose every personal part; a real panic whose message holds a path, a key and a note leaves none of them; ten kept; a folder that cannot be written never makes the hook fail; nine hostile files, a link to /etc/hosts and a link to a well-formed report are never shown), sv159 (the diagnostics endpoint lists a recorded crash and never a planted file), kk-cli dg02 (plain-words rendering) and cp01 (the real binary crashes on purpose through an internal `crash-probe` command: exit status 101, the usual message on the terminal, one report, no leak, version and place right). 11 mutants of the new code: 9 caught, 1 equivalent (the size cap cannot change what is shown, only how much a hostile file could make the reader read), 1 survivor fixed by a new case (a link to a valid report).
- Not covered: `kaku tui`, `kaku telegram`, `kaku mcp` and the desktop app's own process do not install the hook (only `kaku serve`, where the work happens); a crash of the reader, the helper or a sandbox child; a crash by abort, a signal or running out of memory (a panic hook sees none of them); a screen for the report in the web page or terminal; sending a report with the owner's consent (the owner copies the text). A crash in a thread that is not named shows `unnamed`.

## Web report card and a leak found by looking (ledger G-GH-10)
- Settings > Report a problem shows the same text `kaku diagnostics` prints (the renderer moved to `kk-service::report` so both share it), with Copy and Save as a file; nothing is sent. `GET /api/diagnostics` needs the owner's session and returns the text and the data behind it.
- Found in the screenshot of the first browser run: a check's text named `/home/<account>.guest/...` (a worker path from the virtual machine) because only the host's own home and the data folder were replaced. `scrub_home_paths` now replaces the user name in any `/home/NAME/`, `/Users/NAME/` or `C:\\Users\\NAME\\` path, names with spaces included (rp01; sv181 sends another account's path and a spaced name through the service; 3 mutants caught).
- Not covered: a user name that appears outside a home path; a terminal screen for the report.
