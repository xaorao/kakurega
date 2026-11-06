# Desktop shell: Tauri or Electron (Milestone II, first decision)

## Target experience
A native app that opens straight into the workspace, runs the kernel on the user's own machine, shows approvals in a window no agent or page can draw over, and uses the operating system's own authentication (Touch ID on macOS) for the approval signature.

## Strongest references
Electron (Chromium and Node in every app), Tauri v2 (Rust core, system webview, capability-scoped commands), OpenCode desktop (Electron, MIT, `private-ai/inspiration/opencode/packages/desktop`, commit fe04656; an earlier version of this note wrongly said Tauri), Codex app server. Read in full for this note (VERIFIED by reading): `main/index.ts`, `desktop.ts`, `ipc.ts`, `ipc-transport.ts`, `preload/index.ts`, `windows/security.ts`, `service/sidecar-credentials.ts`. NOT read: window creation details beyond the flags, lifecycle, plugins, updater, packaging config.

## Facts gathered (live search, this session)
- OBSERVED: WKWebView (what Tauri uses on macOS) runs in the app's context; passkeys there are limited to the app's associated web domain, and a browser-grade entitlement is reported anecdotally. Tauri-specific WebAuthn support was NOT found.
- OBSERVED: Electron adds a Touch ID platform authenticator only from Electron 42 through `app.configureWebAuthn`, and the in-renderer path needs an https origin that satisfies the passkey's relying-party id; Linux has no native path; a community MIT module (`electron-webauthn-mac`) uses Apple's AuthenticationServices.
- Both facts say the same thing for us: a page-level WebAuthn prompt is the wrong layer for approvals on a loopback app. The approval prompt and key belong in the shell's native layer.

## What OpenCode's Electron shell does well (VERIFIED by reading, MIT, adaptable with credit)
- Windows use `contextIsolation`, `sandbox`, no Node in the renderer; UI served from a private custom scheme.
- Navigation policy: new windows denied, off-origin navigation sent to the system browser; permission allow-list checked against the requesting URL and main frame.
- The renderer never sees the sidecar credential: the main process adds the Authorization header only for the top-level renderer frame (embedded pages in the same session do not inherit it).
- IPC over a transferred MessagePort per window instead of broad `ipcRenderer` exposure; CLI bundled outside the asar; first window shown before the heavy bundle loads.
## Weaknesses to beat
- The IPC port is handed to the window on every `did-finish-load` without checking the frame URL in the code read (a navigated or embedded page could receive it); we check the frame origin and top-level status before handing anything over.
- The main process is a large Effect-TS dependency tree; ours needs a small surface (window, sidecar, native approval).

## Decision (INFERRED, to be proven by a spike before it is final)
Leaning Tauri v2, but the strongest reference being Electron (MIT, forkable patterns above) keeps this OPEN until the spike measures both: Tauri v2, with the kernel crates linked into the Rust core (same language, no second privileged runtime, no Node main process), the existing web app loaded from the companion's loopback origin, and approvals done by a native window plus the OS authentication API through Rust (LocalAuthentication and the Keychain or Secure Enclave on macOS). Reasons: one language and one permission model across kernel and shell; smaller attack surface; capability-scoped IPC; no dependency on WebAuthn support inside a system webview.
Risks to test before committing: LocalAuthentication from a Tauri app without a Developer ID, webview differences across macOS, Linux (WebKitGTK) and Windows, window-overlay and screen-capture protection for the approval window, auto-update signing.
Fallback if the spike fails: Electron 42+ with the native approval module, same kernel as a child process.

## DECISION (final for now): Electron
Measured on this Mac (3 runs, Electron 45.2.2, `/tmp` harness, process tree incl. the companion): app bundle 253 MB; time from launch to the usable workspace 0.73 s, 0.95 s and one cold 3.7 s; memory about 454 MB across 5 processes.
Why Electron over Tauri (reasons that are verified, not guesses):
1. Our whole quality standard is real-UI testing with Playwright, axe and screenshots; Playwright drives Electron natively (13 desktop checks already run this way) and cannot drive a macOS WKWebView shell, so Tauri would leave the desktop UI less testable.
2. The strongest reference shell (OpenCode, MIT) is Electron and its hardening patterns are adoptable with credit.
3. Electron 42+ has a Touch ID platform authenticator and `systemPreferences.promptTouchID` is what the shell uses today; the native approval is working in the real window (stubbed OS prompt; real Touch ID is II-03).
4. One Chromium everywhere: no WebKitGTK versus WKWebView differences to test on Linux and Windows.
Cost accepted: 253 MB bundle and about 450 MB memory. Tauri was NOT built or measured (disk and time spent on the working path), so the claim that Tauri would be smaller is a reference-based expectation, not our measurement. Revisit if size or memory becomes a user complaint.

## Spike acceptance (all met except real Touch ID and the Tauri measurement, see above) (before this is called decided)
1. A Tauri window opens the web app from a loopback companion started by the shell, with pairing automatic and no URL typed.
2. A native approval window shows the exact file list and digest and requires Touch ID; a page script cannot trigger, hide or answer it (hostile test).
3. The shell survives kernel restart and shows recovery state.
4. Build size, start time and memory measured against a minimal Electron shell.

## Not done
Spike not started; source of OpenCode's desktop package not read; Windows and Linux not tested; disk is tight (about 5 GB free), so the spike builds with a shared target directory.


## Second opinion and the comparable Tauri measurement (supersedes the "final for now" decision above)
A reviewer's critique (GPT, pasted by the owner; treated as outside input, claims checked) made two corrections and asked for one fair comparison:
1. Tauri on macOS CAN be automated: the official WebdriverIO docs (webdriver.io/docs/desktop-testing/tauri/platform-support, OBSERVED through a search of the official pages, not run here) describe an embedded WebDriver provider through the `tauri-plugin-wdio-webdriver` crate, registered only in debug builds. My earlier claim that Tauri would be less testable on macOS was wrong in that form; the remaining cost is rewriting the shell checks in WebdriverIO. NOT tried.
2. `promptTouchID` proves user presence only. It does not bind consent to the exact release. Our flow ties the prompt text to the digest and the owner's confirmation to the typed or supplied digest prefix, but there is no cryptographic approval signature over recipient, bytes, policy version and a nonce with an OS-protected key. That is a real gap for either shell and is now a pending row (II-12).

Measured on the same Apple-silicon Mac, service already running, same web app, same loopback companion, 5 runs each (`spikes/tauri-shell`, Tauri 2.12.5, release profile; Electron 45.2.2):

| | Tauri v2 (system WKWebView) | Electron 45 |
|---|---|---|
| shell executable / bundle | 3.4 MB binary | 253 MB app |
| time until the page's load event | median 260 ms (250 to 317) | median 229 ms (222 to 231; first run 968 ms cold) |
| resident memory after 3 s (app and its helper processes) | median 202 MB, 4 processes | median 440 MB, 4 processes |

Method notes: memory is `ps` resident size of the app and its WebKit XPC helpers (new since launch) for Tauri, and of the process tree for Electron; load time is the app's own start-to-load-event for Tauri and `performance.timing.loadEventEnd` minus the launch call for Electron. A first Tauri run showed 1.5 GB: that was my own bug (an observer that re-triggered itself by setting the title), removed and remeasured. Not measured: packaged .app or .dmg sizes, battery, long-run memory, update recovery, Windows and Linux webviews.

What the Tauri spike has: a window on the loopback app with an origin-locked navigation handler, no capability granted to any window (a page can call no native command), and a native approval dialog that approves or refuses through the same admin API. What it does NOT have: Touch ID, signed approvals, hardening review of a packaged build.

### Evidence-based reading (owner decides: II-04)
- Efficiency: Tauri is about 75 times smaller and uses about 2.2 times less memory with equal perceived start-up. That is material for an app people leave running.
- Boundary: with no capability granted, a page in the Tauri webview has no IPC at all; the shell is Rust like the kernel, with no second runtime. Electron needs its sandbox, preload, fuse and port-handoff discipline to reach a comparable position.
- Cost of switching: re-do the 13 shell checks in WebdriverIO, add the native approval and Touch ID path (needed for both shells), and accept WKWebView differences, with Windows and Linux webviews untested.
- Recommendation: move the shipping shell to Tauri v2 once it passes the same boundary checks and the approval binding (II-12) is built in Rust, keeping the Electron spike as the reference until then. This is a recommendation from measurement, not a decision: the owner accepts or rejects it.
- Kernel and approval contracts stay framework-independent (`/admin/approvals` and the digest), so the choice can be changed without rebuilding the security architecture.

## Same attacks on both candidates (II-13)

Both shells were run through the same renderer-compromise script (session cookie, admin endpoints, CSP, native-command access, window.open, navigation). Electron: `node tests/e2e/desktop_attack.mjs` (19 checks) driven through Playwright. Tauri: `python3 tests/e2e/tauri_attack.py` (9 checks) driven by a test-only feature that evaluates the script inside the real WKWebView and reports through the URL fragment (the window title does not mirror document.title).

- Result: the page is equally contained in both. Every admin endpoint refuses it, the CSP blocks inline script, eval, remote scripts and beacons, no window opens, and navigation is refused or reversed. In Tauri the page can see `__TAURI_INTERNALS__` but every native command (dialog, window, app, fs, an unknown command) is refused because no capability grants anything.
- Difference found: Electron let a page navigate to `about:blank` (fixed with a did-navigate guard); Tauri's `on_navigation` refused it by default.
- Not equal yet: Electron has asar integrity validation, fuses and a tested tamper response (II-13b, II-13c). Tauri has no counterpart built or tested (code signing of the binary, integrity of the frontend, update path). That gap, not the attack surface of the page, is what remains to compare.
- The decision (II-04) is still the owner's.

## Update (retrospective audit, matrix F E2/E4/E9, extension X2)
- Re-read `main.mjs`: the matrix said the permission check handler was missing, but it was already set; what was missing is the device handler. Now `setDevicePermissionHandler` refuses, `will-redirect` is stopped for off-origin targets, and a single-instance lock makes a second launch exit while the first window comes forward.
- Evidence (Linux VM, real Electron): the page cannot be granted location, camera, microphone, USB, MIDI, clipboard or notifications; a second launch exits at once (removing the lock makes that check fail). Attack suite 23/23, no-Touch-ID flow 7/7.
- Extension: the selection is checked against the same provider table as the service (`formats.js` generated from `kk-secrets`, drift test in the Rust crate) before anything is sent to localhost. Tests xs01-xs03; mutations of the scanner and of the generated table fail tests.
- Still open from matrix F: signing and notarisation (needs an Apple Developer ID, deferred by the owner), auto-update with a verified publisher, per-launch secret for the loopback origin, mandatory approver key in packaged builds.

## Crash recovery (added)
- Companion exit while the app is open: restarted up to 3 times in 5 minutes with growing waits (0.5 s, 1 s, 2 s). The companion takes a new port each start, so the shell re-reads `admin.json`, asks for a new pairing code and loads the window with it; the approval poller reads the current connection. Past the limit the owner gets a plain message and nothing restarts. Anything running at the crash is stopped (job state is not resumed); the owner is told by a system notification, nothing was sent anywhere.
- Window process crash (`render-process-gone`, not a clean exit): the window is reloaded (same session, no new login); the same 3-in-5-minutes limit then a message. An unresponsive window asks Wait or Reload.
- Evidence: ds10-ds11 (policy), `tests/e2e/desktop_recovery.mjs` on real Electron on the Mac (7/7) and in the Linux VM (7/7): kill -9 of the companion, `forcefullyCrashRenderer`, repeated kills to the limit. Mutations: no reload on renderer crash fails the run; removing the limit fails two checks.
- Not covered: Windows; a hung (not dead) companion; resuming an interrupted review; the unresponsive dialog (not driven).
- Harness note: `desktop.mjs` needs `spikes/enclave/build.sh --test target/debug/kaku-enclave-test`; without it two enclave checks fail (precondition, not a product defect).

## Per-launch shell secret and pairing throttle (ledger F-E6)
- Reference: OpenCode `sidecar-credentials.ts` pattern (read earlier, trace row on the desktop shell; MIT): the shell main process holds a per-launch credential and adds it to its own window's requests. Not copied; rebuilt for our companion.
- Found by reading our own gate (VERIFIED by a failing test first): after 5 wrong pairing codes the companion refused even the RIGHT code for 60 s, so any program on the computer could keep the owner out of their own app by sending wrong codes in a loop. The code is 128 random bits, so the lockout never protected anything. Now the code is checked first; only wrong guesses are throttled; a test (ws13c) guards the 128-bit premise, because shortening the code would make this unsafe.
- Decision: BUILD. The desktop shell generates 256 random bits for every companion it starts, gives them to `kaku serve --shell-secret-stdin` on stdin (not the environment or arguments, which review workers inherit and other programs can list), and adds `x-kk-shell` only to requests to the companion's own origin (policy.mjs `withShellSecret`; a lookalike origin or a page-supplied header never gets or keeps it). The companion answers 403 `SHELL_ONLY` to anything else except `/admin/` (its own bearer token) and `/ext/` (its own pairing). Constant-time compare; at least 32 characters or the companion will not start.
- What it buys: another program on the computer cannot load the page, probe `/api/*`, try the pairing door, or use a stolen session cookie. What it does NOT buy: a program running as the same user can still read `admin.json` and mint its own pairing through the admin door; that is the same trust domain and the owner's operating system account is the boundary.
- Evidence: kk-companion ws13b, ws13c, ws20–ws22 (13 mutants caught, one equivalent in serve), desktop unit ds20–ds21, real Electron: desktop.mjs 22 checks including five outside-program refusals, desktop_attack.mjs 23/23 (its old control "a process with the cookie is served" now asserts refusal), desktop_recovery 7/7, desktop_packaged 7/7, desktop_fuses 7/7, Linux in the VM (`scripts/desktop-linux`) all pass; 3 Electron-wiring mutants caught.
- Not covered: Windows desktop; the browser workspace (`kaku serve` for a normal browser) has no shell secret by design, so its protection is still the pairing link, cookie and Origin checks. The attack suite's later cookie attacks now meet `SHELL_ONLY` first; the underlying Origin, CSRF and cross-site checks remain proven by the companion tests and the browser e2e.

## A companion no longer outlives a desktop app that was killed (found while testing F-E6)
- Found: a `kaku serve` started by the desktop shell was still running two hours after its test Electron had been killed. Before the shell secret the companion had no way to notice its shell had died (`KAKU_EXIT_WITH_PARENT` was never set by the shell), so a crash, a forced quit or a power cut left a service running with nobody holding its window. This is older than the shell secret; it was seen because the test debris was still there.
- Fix: the shell keeps its end of the pipe that carried the secret open for its whole life. `kaku serve --shell-secret-stdin` reads the secret, then keeps reading the pipe; when it reaches the end (the shell is gone, however it went) the service removes its admin file and exits (within a second). Tests: kk-cli ss02 (stays while the pipe is open, leaves and removes the admin file when it closes), `tests/e2e/desktop_forcequit.mjs` (real Electron on this Mac and in the Linux VM: the same companion keeps running for five seconds while the app is open; after SIGKILL of the app it is gone in about one second and so is `admin.json`). The mutant "the service ignores a closed pipe" is caught by both. The mutant "the shell closes the pipe right after the secret" is equivalent under Electron: Electron's half-closed stdio is not seen by the child, so it is not detectable there; the pipe closing when the process dies is what the test proves.
- Not covered: a shell that is stopped (SIGSTOP) rather than killed; Windows.
