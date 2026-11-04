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
