# Telegram bridge (registry N53 partly, N68 deliberately not; Milestone III)

## Target experience
A message on the owner's phone when a review finishes or an approval is waiting, and two commands to look at the state from anywhere. Never the content of anything.

## The decision that matters: Telegram sees what it is sent
A chat service is a third party that stores messages. The surface matrix already says chat services are untrusted transports and that consequential approvals need a passkey with an exact-bytes summary on a trusted device. Showing the exact files of a release in a Telegram message would hand the very bytes the owner is trying to protect to a third party. So the bridge never carries source text, findings, memory text, chat text, recipient names, folder paths or release contents, and it has no approve, deliver, chat or memory command. Approval stays on the desktop window, the terminal workspace or `kaku approve`. A phone approval needs a native app with a passkey; that is NOT built and is not faked with a chat button.

## What it does
- One-time pairing: `kaku telegram pair` prints a code; sending `/pair CODE` to the bot binds that chat. After that only that chat id is served; every other chat gets no reply at all, so the bot is not an oracle.
- Commands: `/status` (counts), `/reviews` (ids and states), `/help`.
- Notifications: a review finished (state and counts), an approval is waiting ("decide on your computer"). Ids only.
- The bot token is read from the environment or a `.secrets` file, is never printed, and is scrubbed from any error text.

## Reading record
No Telegram bridge code was read for this component. The Bot API behaviour used (`getUpdates` long polling, `sendMessage`) comes from the official Bot API documentation and is exercised against a local fake server; real Telegram behaviour was NOT verified end to end because the owner's chat id is not known (HUMAN.md).

## Quality criteria
Pure bridge tests (pairing, allowlist, silence to strangers, templates carry no private text, notification de-duplication, token scrubbing, message limits) and a live run of `kaku telegram` against a real service and a fake Telegram server.

## Not covered
Real Telegram delivery, group chats, webhooks, media, rate limits imposed by Telegram, other chat apps, phone approvals.

## Update after the real run and the capability matrix (`capability-matrix/C-channels-telegram.md`)
- Fixed (validated by reading and by tests tg16, tg17): the HTTP client was built without TLS; the pairing code was read once at start; a wrong or expired code got silence; send ignored HTTP status so a 429, 403 or 5xx lost the notification silently; a re-pair did not stop a running bridge from notifying the old chat; getUpdates 401 and 409 were not distinguished; the pairing code was 32 bits in a world-readable file (now 48 bits, mode 0600, five tries).
- Delivery now reads the answer, honours flood control, retries temporary failures with back-off from a bounded outbox (50 messages, 10 tries) and drops messages Telegram will never accept.
- Reading owed (stated in the matrix): IronClaw's pairing and egress source was not in the checkout; Ghast's bridge is in a closed app. Capability recovery from the matrix continues in priority order.

## Update (retrospective audit, matrix C rows 3.4 and 3.7)
- Built: a flood limit (at most five commands from the bound chat in ten seconds, then silence and no admin requests; strangers do not count) tg20; the phone's `/` menu is set once at start to exactly the three commands the bridge answers (`setMyCommands`; MENU test tg21 and a fake-server check in `telegram_fake.py`); `kaku telegram doctor` (token file is a regular, non-linked file readable only by the owner; Telegram accepts the token; no webhook; chat paired; no stale code) and the bridge now refuses a linked token file (tg22).
- Still not built: the status file and `kaku telegram status`, non-consequential buttons, using the vault for the token.

- Built (approval-notice hygiene): the bridge used to treat any approval the service reported (including approved and denied ones) as "waiting"; it now announces only the state `waiting`. A notice it sent is remembered by message id for this run (at most 100); when that approval stops waiting, the same message is edited to say so, once (tg24-tg26, `telegram_fake.py` 28/28 with a fake `editMessageText`). After a bridge restart an older notice cannot be edited (no id), so nothing is sent for it. Mutations of the state rule and the untracked-edit skip fail tests.

## Mute (ledger C-26)
- Reference: the matrix row (OpenClaw and Hermes bridges; their sources are NOT READ for this row) and the Telegram Bot API field `disable_notification` (a Boolean parameter of `sendMessage`, confirmed on core.telegram.org/bots/api; its exact semantics, a message delivered without sound, rests on the documentation and was not tried against the live service: NOT VERIFIED).
- Behaviour: in the bound chat, `/mute 30m` or `/mute 2h` (1 minute to 24 hours, a unit is required) makes every later notice arrive without sound or vibration; nothing is held back or dropped, so an approval that is waiting still arrives. `/mute off` ends it, an elapsed mute ends itself, and an unclear ask changes nothing and says how to ask. The reply to the owner's own command is not quiet. The state lives only in the running bridge: a restart ends it, and it cannot widen access, because only the bound chat's own commands are answered and a mute only quiets messages that chat already receives.
- Evidence: tg62 (parsing, bounds, units, injection text refused), tg63 (quiet while muted, loud after off and after expiry, own replies loud, unclear asks), tg64 (another chat and an unpaired bridge get silence and change nothing), tg65 (the request body carries `disable_notification` true or false), tg21 (the menu lists exactly the commands answered). 7 mutants caught.
- Not covered: pacing per chat (C-25, still owed), a live Telegram chat.

## Flood gate and pacing (ledger C-25, with C-11 and C-12 reading)
- Reference: OpenClaw (MIT, commit f38495f7d) `extensions/telegram/src/account-throttler.ts` (625 lines) and `polling-session-restart-policy.ts` (53 lines), both FULL. Learned: a 429's `retry_after` is a penalty on the whole bot token, so one gate holds every call for it, a shorter concurrent answer never shortens it, final replies retry inside a five-minute budget while replaceable updates (stream previews, typing) yield instead of queueing, group chats get their own lanes, and the polling restart backoff starts at 30 s, doubles to ten minutes with 20 percent jitter. Not read: `grammy-api-throttler.ts`, `network-errors.ts`, `sendchataction-401-backoff.ts`.
- A defect found by comparing: our outbox paused only the message that was refused and the rest of that flush; the next flush two seconds later tried the next message while Telegram's penalty was still running, which draws more 429s. Fixed: a gate closes the whole outbox until the deadline (tg66, which fails on the old behaviour).
- Added from C-25: a pacing bucket of three messages at once refilled at one a second (tg67: burst, steady rate, an hour of quiet refills no more than the burst, order kept). Adapted rather than copied: OpenClaw paces group chats; our bridge has one private chat, so the bucket is per bridge.
- Not taken, with reasons: replaceable-update skipping (we send no previews or typing), the five-minute delivery budget (the outbox already gives up after ten tries and 50 messages), a digest of many events (the row asks for one when more than five arrive; not built, owed), 30 s to ten minute restart backoff (ours is for the polling loop and starts at 2 s; a person waits on a notice, and a poll is cheap).
- Evidence: tg66, tg67 and the earlier tg17 (bounds, adjusted for pacing); 5 mutants caught (one was equivalent code and was simplified).
- Not covered: a live Telegram bot and a real 429.

## Pairing names the bot (ledger C-7)
- Reference: the matrix row (call `getMe` at pairing to learn the username and print the deep link); OpenClaw's pairing source NOT READ for this row. Telegram's own rule for usernames (five to thirty-two letters, digits or underscores) is used as the filter.
- Behaviour: `kaku telegram pair` asks Telegram who the token belongs to (`getMe`, 8 second limit, the library's proxy default) and prints `Open your bot at https://t.me/<username> and send it this in a private chat within 5 minutes: /pair CODE`. The link is made only from a plain username; any failure (no token at hand, unreachable, an error status, junk, `ok: false`, no username, an odd username) leaves the old wording without a link, and pairing works exactly as before.
- Evidence: tg68 (the link filter: ten hostile or malformed names refused), tg69 (a real answer, 401, junk, `ok: false`, a missing username, an error status that carries a username, an unreachable server). 6 mutants caught.
- Not covered: a QR code (the matrix row names one; not built: needs a QR encoder dependency and a read of its licence), the whole `kaku telegram pair` command against a mock Telegram (it needs a running service first), a live bot.

## Approval notice names where to decide (ledger C-36)
- Reference: the matrix row only (Ghast's trusted-device handoff names the review and the surface); no source exists to read. The text change is ours.
- The notice now reads: "An approval for <review id> is waiting on your computer. Decide there: open the Kakurega workspace (Waiting for you), or run `kaku approve` in a terminal. Nothing is sent until you approve there." Every named place was checked to exist: the web navigation entry "Waiting for you" (`nav.inbox`) and the `kaku approve` command (usage in `main.rs`). The id is the review id reduced to safe characters, as before; the notice still carries no recipient, path, file name or text, and still cannot approve anything.
- Evidence: tg03 asserts the new wording and, as before, that no recipient name appears. Not covered: a live Telegram chat; the desktop shell's own wording (it shows the same web page).

## Long polling on its own thread (ledger C-11)
- References (matrix C, read as recorded there): Goose `POLL_TIMEOUT_SECS=30` with a client timeout of 40 (`telegram.rs` 14, 744-770), Privateer 50 s with 60, OpenClaw isolating `getUpdates` in a worker. Not re-read for this change.
- Before: one loop made a request with `timeout=2` about every two seconds forever, and everything else (notices, the outbox, status) ran in that loop. A longer hold in that loop would have delayed notices by up to the hold.
- Built (`kk-cli/src/telegram.rs`): a polling thread asks Telegram to hold each poll open for 25 seconds (`LONG_POLL_SECS`) with a 35 second client timeout, and hands every answer to the main loop over a channel; the main loop wakes at least every 500 ms to announce reviews and approvals, flush the outbox and write the status. The offset moves past a batch only after the batch was handed over. Failures back off in the poller with a jittered growing pause as before; a 409 is reported once and retried every 10 s; a 401 stops the bridge with a message that does not contain the token.
- Measured against a fake Telegram that holds polls like the real one (`tests/e2e/telegram_fake.py`, now 34 checks): every poll asks for a 25 s hold; an idle bridge made 0 requests in twelve seconds (it was about six); a message arriving during a held poll is answered in 0.2 s; an approval notice is sent 2.1 s after it is requested, not after the hold; a rejected token exits with the message; a conflict is said once and the bridge keeps running. 7 mutants: 5 caught, 1 equivalent (the poller carries on after a 401 but the process is already ending), 1 did not build.
- Found on the way: that script was part of `scripts/e2e-all` but not of the gate I had been running, and had gone stale when `/mute` was added to the phone's menu (it still expected three commands). Fixed, and the gate now runs it with the other scripts that were left out.
- NOT VERIFIED: against the real Telegram servers (no bot token here), including that 25 seconds is accepted through every real network path; behaviour when the computer sleeps mid-poll.
