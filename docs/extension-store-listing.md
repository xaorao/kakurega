# Kakurega browser extension: store listing text

What to enter in the Chrome Web Store developer dashboard. Every statement here is checked against the code by `apps/extension/test/policy.test.mjs`.

## Single purpose

Send text you select on a web page to your private Kakurega workspace on this computer.

## Short description (the manifest `description`)

Send a text selection to your private Kakurega workspace on this computer. Nothing is read from a page unless you choose Send.

## Detailed description

Kakurega keeps your notes and reviews private on your own computer. This extension adds one right-click item, **Send selection to Kakurega**, for text you have selected on a web page.

What it sends, only when you choose that item: the text you selected (up to 2,000 characters; text that looks like an API key or token is refused and never sent) and the website it came from (the site name only, never the page path or search terms). It sends them to the Kakurega program on this same computer and to nowhere else. Nothing is saved until you review it in Kakurega and accept it.

It needs Kakurega installed and running on this computer, and a one-time pairing. It has no analytics, no ads, no tracking and loads no code from the internet.

## Permission justifications (dashboard fields)

- `contextMenus`: adds the single right-click item **Send selection to Kakurega** for selected text.
- `storage`: saves the port of the Kakurega program on this computer and the pairing token, in local extension storage only.
- Host permission `http://127.0.0.1/*`: reaches the Kakurega program that runs on this same computer. No other address is requested or reachable.

## Data use answers (privacy practices tab)

- Handles user data: yes. Type: website content (the text the user selects) and the site name it came from.
- Collected only on a user action; used only for the extension's single purpose; transmitted only to the program on the user's own computer (127.0.0.1); not sold; not used for advertising, creditworthiness or unrelated purposes; not shared with a third party.
- Remote code: no.
- Privacy policy: the text of `docs/extension-privacy-policy.md`, published at a public address.

## Still to supply by a person (not in this repository)

Store icon (128 by 128) and screenshots; a public address for the privacy policy; the publisher account with 2-Step Verification on.
