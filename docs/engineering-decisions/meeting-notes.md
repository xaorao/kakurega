# Meeting notes from a transcript

1. Experience a user expects: paste or drop a transcript, get the decisions, who does what by when, and what is still open, and be able to trust each line without reading the whole transcript again.
2. References: Granola, Otter and Fireflies (closed; known from use and their public pages, not studied for this) and the hackathon rival Aftercall (private call notes; REPORTED from `ARCHITECTURE/07_COMPETITOR_FIELD.md`, its clone is no longer on disk so its code was NOT READ for this). None of them shows, per point, the words it rests on with a check that those words are in the transcript.
3. Approach: independent build on what the product already has. The answering model writes strict JSON; every point must carry a quote; a point is listed as checked only when that quote is in the transcript (compared without regard to spacing or case), with the line it starts on. Anything else is listed apart as a guess. Same idea as the checked quotes of ask-your-documents (`crates/kk-review/src/ask.rs`).
4. Quality bar: no invented point among the checked ones; a secret in the transcript never reaches a model; names and personal values are hidden from a model elsewhere; the limits are printed with the result.

## Built
- `crates/kk-review/src/meeting.rs`: the instruction (the transcript is marked as untrusted data), strict parsing (an object with only the four known fields), the quote check, cutting a long transcript into at most 8 parts of whole lines, putting parts together without repeats, text output.
- `crates/kk-service/src/service/meeting.rs`: `POST /api/meeting-notes` in the owner's session. A line that holds a secret is replaced in place before anything is sent (line numbers stay the transcript's). The question goes through the hiding layer, so a model elsewhere gets placeholders and the quotes are checked against the real words. The signed log gets counts, never text.
- Web: the Meeting notes screen (paste, or a .txt, .md, .vtt or .srt file; subtitle counters and timings are dropped), checked points with their line, guesses apart, copy, save as a file, keep as a confidential note. Terminal: `kaku notes FILE [--title] [--out] [--json]`, `-` for standard input.

## Evidence
- kk-review mn01 to mn05; 8 of 8 applied mutants of the checks caught (one more could not be applied as written; its case, an unknown field, is in mn03).
- kk-service sv250: a model on this computer and a recording model elsewhere; the invented action is never among the checked points; the key line is never sent; with names listed, the model elsewhere sees neither name and the quotes still check.
- `tests/e2e/notes_cli.py` 12 checks with the real binary; `tests/e2e/meeting_web.mjs` 13 browser checks (subtitle file, lines, guesses apart, keep, save, phone width, no console errors).
- One run with a real model (the outside test route, Sonnet) on a 14-line transcript of a three-person meeting: 6 points, all 6 checked, owners and times right, 2 personal values sent as placeholders. One run is not a quality measurement; the three-run comparison against a local model is owed to the test phase.

## Limits
- Checked means the quoted words were said. It does not mean the point reads them correctly, and something said can be missing from the notes.
- A password spoken in plain words ("it's still hunter2") is not recognised as a secret and is sent like any other line.
- Audio is not transcribed here; the transcript comes from the user.
- A transcript longer than about 190,000 characters is refused with words; split it.
