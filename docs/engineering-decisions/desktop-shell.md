# Desktop shell: Tauri or Electron (Milestone II, first decision)

## Target experience
A native app that opens straight into the workspace, runs the kernel on the user's own machine, shows approvals in a window no agent or page can draw over, and uses the operating system's own authentication (Touch ID on macOS) for the approval signature.

## Strongest references
Electron (Chromium and Node in every app), Tauri v2 (Rust core, system webview, capability-scoped commands), OpenCode desktop (Tauri, MIT, in `private-ai/inspiration/opencode`, not yet read), Codex app server. Source reading of those shells is OWED before building and is not claimed here.

## Facts gathered (live search, this session)
- OBSERVED: WKWebView (what Tauri uses on macOS) runs in the app's context; passkeys there are limited to the app's associated web domain, and a browser-grade entitlement is reported anecdotally. Tauri-specific WebAuthn support was NOT found.
- OBSERVED: Electron adds a Touch ID platform authenticator only from Electron 42 through `app.configureWebAuthn`, and the in-renderer path needs an https origin that satisfies the passkey's relying-party id; Linux has no native path; a community MIT module (`electron-webauthn-mac`) uses Apple's AuthenticationServices.
- Both facts say the same thing for us: a page-level WebAuthn prompt is the wrong layer for approvals on a loopback app. The approval prompt and key belong in the shell's native layer.

## Decision (INFERRED, to be proven by a spike before it is final)
Tauri v2, with the kernel crates linked into the Rust core (same language, no second privileged runtime, no Node main process), the existing web app loaded from the companion's loopback origin, and approvals done by a native window plus the OS authentication API through Rust (LocalAuthentication and the Keychain or Secure Enclave on macOS). Reasons: one language and one permission model across kernel and shell; smaller attack surface; capability-scoped IPC; no dependency on WebAuthn support inside a system webview.
Risks to test before committing: LocalAuthentication from a Tauri app without a Developer ID, webview differences across macOS, Linux (WebKitGTK) and Windows, window-overlay and screen-capture protection for the approval window, auto-update signing.
Fallback if the spike fails: Electron 42+ with the native approval module, same kernel as a child process.

## Spike acceptance (before this is called decided)
1. A Tauri window opens the web app from a loopback companion started by the shell, with pairing automatic and no URL typed.
2. A native approval window shows the exact file list and digest and requires Touch ID; a page script cannot trigger, hide or answer it (hostile test).
3. The shell survives kernel restart and shows recovery state.
4. Build size, start time and memory measured against a minimal Electron shell.

## Not done
Spike not started; source of OpenCode's desktop package not read; Windows and Linux not tested; disk is tight (about 5 GB free), so the spike builds with a shared target directory.
