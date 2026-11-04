# Browser extension (registry N67; Milestone III)

## Target experience
Select text on any page, right-click, choose "Send selection to Kakurega"; it appears in Kakurega as a suggestion marked untrusted, with only the site's origin as its source. Nothing is saved until the owner accepts it. The extension asks the browser for almost nothing and can do almost nothing, so installing it is not a leap of trust.

## References and what was read
- Ghast extension 0.1.9 (no licence file: ideas only; `private-ai/ghast-ai/_shipped`): its manifest was read. It requests `tabs`, `scripting`, `nativeMessaging`, `identity`, `bookmarks`, `notifications`, `unlimitedStorage`, about 30 host permissions including `localhost` and `127.0.0.1`, and injects a content script into 15 sites; its background script is 6.9 MB. Source is minified and was not read. What it teaches: the permission surface of a capable extension is large and every item is trust the user must extend. We copy none of it.
- Not studied: other extensions' source, Chrome's extension security review guidance beyond the manifest reference. NOT VERIFIED: Chrome Web Store review requirements.

## Decision
The smallest useful extension: permissions `contextMenus` and `storage` only; host permission `http://127.0.0.1/*` only; no content scripts, no `activeTab`, no `tabs`, no `scripting`, no remote code, no `externally_connectable`. The selection text and page URL come from the context-menu event itself, so the extension never reads a page it was not told to send from.

The service side is a separate channel `/ext/*`: a bearer token minted by a one-time pairing code that the owner generates (`kaku extension pair`), pinned to the extension's own origin, refused if a cookie or any non-extension Origin is present, rate-limited on wrong codes, and able to do exactly two things: report counts and queue an untrusted memory suggestion (text up to 2000 characters, origin only, secrets refused). The owner accepts or dismisses in the Memory screen. Pairing again replaces the old extension; the owner can revoke at any time.

## Quality criteria and evidence
`cargo test -p kk-service --test flow sv18` (pairing, origin pinning, token, cookies, lockout, validation, owner acceptance, restart, revoke; seven deliberate breaks each failed it), `cd apps/extension && node --test test/*.test.mjs` (10 tests: exact permission set, content security policy, no eval or inline code, loopback only, pure logic, background wiring with a stubbed browser), `node tests/e2e/extension.mjs` (real Chrome: the extension loads, pairs, queues, the owner reviews it in the workspace, a hostile web page cannot read or plant anything, disconnect and revoke).

## Not covered
The native context-menu click itself (no automation hook; the handler is tested with a stubbed browser and the same capture module is driven from an extension page against the real service); Firefox and Safari; Chrome Web Store packaging and review; an extension update path; capturing page content other than a selection; any use on pages loaded over a non-loopback service (there is none by design).
