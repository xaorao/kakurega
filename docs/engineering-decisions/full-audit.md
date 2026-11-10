# Full audit: the screen matrix (QA guide Part 4.5, section SCREEN)

Written before and during the audit, as the guide asks. Phase 1 of the plan "audit, fix, full test, polish".

## Inventory (counted by reading the code)
- Web pages: 11 (new review, waiting, documents, chat, memory, instructions, settings, privacy, agent access, activity, a review with five tabs).
- Interactive elements counted at runtime on a desktop page in English, one per page in the order above: 24, 20, 21, 37, 41, 20, 31, 17, 21, 26, 26 (263 in all). A change in these counts means the inventory was wrong.
- Other doors to the same code: 113 service routes (REST and admin), 24 `kaku` subcommands, about 60 terminal commands, the MCP tools, the Telegram commands, the browser extension, the desktop shell.

## What was run
- `tests/e2e/audit_matrix.mjs`: every page x {desktop 1280, phone 390} x {light, dark} x {English, Hindi} = 88 cells, with a finished review, a kept conversation and a turn in them. For each cell: the interactive count, sideways overflow, targets under 24 px, accessibility violations (WCAG 2.0 to 2.2 A and AA), console and page errors, failed requests, and a screenshot that I read myself (phone dark English for every page, Hindi phone and desktop samples).
- A first run showed the Hindi cells were English (the language is read when the page starts); the tool was fixed and the Hindi cells re-run.

## Defects found
- D1 (high) Phone: the whole navigation (ten links, a search box and the review list, about 700 px) stood above the content on every page, so the page's own heading started below the first screen. FIXED: the navigation is closed on a narrow screen until the Menu button is pressed, closes when a page is chosen, and the search shortcut opens it. Browser checks added (menu closed at first and the heading on the first screen, the button says it is expanded, a page choice closes it).
- D2 (medium) Phone: the header's four chips stacked to about 190 px. FIXED: smaller type and a tighter header, about 165 px with the chips under the brand.
- D3 (high) Phone, review page: the row of five tabs was wider than the screen and widened the whole page (464 px on a 390 px screen). FIXED: the tab row scrolls inside itself; a browser check that the page is not wider than the screen.
- D4 (low) Targets under 24 px (WCAG 2.2 AA 2.5.8): the collapsible section headers (23 px), date boxes (21 px) and checkboxes (13 px). FIXED: 28 px headers, 36 px date and number boxes, 24 px checkboxes.
- D5 (low, open) Phone, the release table: file names and hashes break in the middle of a word. Readable, not pretty; a stacked layout would be better.
- D6 (design note, open) Hindi: text the service writes stays English: the readiness checks ("Ready Model ..."), the release notice's statements ("That the private source repository is the one that was reviewed ..."), "1 withheld file(s)", the route chip "sandbox in the kak virtual machine". The release statements are the signed wording and must not change; the others could be translated.

- D7 (medium, copy) Counts of exactly one read wrongly: "1 entries" on a fresh Activity page, "1 passages" on a picture card, and the same for "1 files", "1 notes", "1 messages", "1 hours", "1 bytes". FIXED: a text may now have a singular form (`key.one`, chosen when the count is exactly one; `src/plural.ts`, unit tests pl01 and pl02), added for ten texts in English and Hindi. The audit tool now checks every page for "1 <plural noun>" and for an unfilled `{placeholder}`; with the singular text removed it reports "activity desktop dark en: copy: 1 entries", so the check works.
- Empty states (80 cells: ten pages with nothing in them) were run and read: 0 findings by machine; the pages say plainly what is empty.

## Result
- After the fixes: 0 sideways overflow, 0 accessibility violations, 0 console or page errors, 0 failed requests in all 88 cells; the only machine finding left was the checkbox size, fixed afterwards.

## Not covered (phase 1 continues)
- Terminal screens on small and large terminals; `kaku` subcommands with real models; the MCP server with a real client; Telegram with a real bot; the desktop and extension; keyboard-only use of every page; a screen reader; states other than "populated" (empty, error, loading) for every page in the matrix.
