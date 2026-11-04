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
