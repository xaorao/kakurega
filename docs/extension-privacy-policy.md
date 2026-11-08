# Kakurega browser extension: privacy policy

This policy describes the Kakurega browser extension for Chrome. It covers only the extension. The Kakurega program it connects to has its own documentation.

## What the extension does

When you select text on a web page, right-click, and choose **Send selection to Kakurega**, the extension sends that text to the Kakurega program running on your own computer. It does nothing else with a page. It does not read a page until you choose Send, and it has no access to your tabs, history, bookmarks, cookies or passwords.

## What it collects, and when

Only when you choose **Send selection to Kakurega**, the extension collects and sends two things:

1. **The text you selected**, up to 2,000 characters. Text that looks like a secret (for example an API key or access token) is not sent at all, and the extension says so without repeating it.
2. **The website the text came from**, as its address up to the site name only (for example `https://wiki.example.com/`). The rest of the address (the page path, any search terms and anything after them) is never read or sent.

The extension also saves two settings in your browser's local extension storage: the port number of the Kakurega program on your computer and a pairing token you create when you connect the extension. They stay on your computer and are not synced to your Google account.

## Where it goes

Only to `127.0.0.1`, which is your own computer, to the Kakurega program you paired the extension with. The extension is not able to contact any other address: its permissions name only that one, and it refuses to follow a redirect. Nothing is sent to the extension's developer, to a server, to an advertiser or to any other third party. There is no analytics, no tracking and no advertising in the extension.

## What happens to the text afterwards

The text is placed in a waiting list inside Kakurega. Nothing is saved as a note until you review it there and accept it. The extension itself keeps no copy of the text after sending it.

## How the data is used and shared

The data is used only for the single purpose above: getting text you chose from a web page into your Kakurega workspace. It is not sold, not used for advertising or credit decisions, not used to build profiles and not shared with anyone. No person at the developer reads it.

## Security

The connection never leaves your computer, so it does not cross a network. The pairing token is required for every request and can be revoked from Kakurega at any time. The extension loads no code from the internet and evaluates no text it receives as code.

## Your choices

- Do not choose Send, and nothing is read or sent.
- To stop the extension talking to Kakurega, revoke the pairing in Kakurega, or remove the extension; removing it deletes its saved port and token.
- To delete text you already sent, discard it from the waiting list in Kakurega, or forget the note once accepted.

## Changes

If a future version changes what is collected or where it goes, this policy will be updated first and the change will be stated on the extension's store page before the update is published.

## Contact

Use the contact address shown on this extension's Chrome Web Store page.
