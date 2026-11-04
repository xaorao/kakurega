# Offline document parsing (experiment X8)

## Target experience
A private PDF, spreadsheet or contract is read on the person's own machine, every claim points back to a page, sheet or cell they can open, and no byte of the document goes anywhere.

## References
Docling 2.146.0 (MIT, local clone commit 0634e2f for reading; installed from PyPI for the run), its layout model (docling-layout-heron) and TableFormer; markitdown, Unstructured and the AnythingLLM collector were compared on paper in `SOURCE_COVERAGE_BY_COMPONENT.md` and not run here.

## Approach under test
Models are downloaded once (`docling-tools models download`, 1.3 GB) into a local artifacts directory. Parsing then runs with `HF_HUB_OFFLINE=1` inside a macOS Seatbelt profile that denies all network (`sandbox-exec`), plus an in-process recorder that logs and blocks every socket connect and DNS lookup. Seeded documents with known ground truth: a 3-page PDF (text on pages 1 and 3, an 11x4 table on page 2), a second PDF with a 4x4 table, a DOCX with a table, an XLSX with two sheets, an HTML page with a table. Anchors are scored as: text found, and page (PDF) or row/column (tables) correct.

## Results (VERIFIED_BY_RUN, `kakurega/tests/x8-offline-parse/run.sh`)
- 95 of 95 seeded items found with the correct anchor: DOCX 7, HTML 5, XLSX 21 (sheet cells), PDF 62 (60 table cells, 2 text paragraphs with the right page).
- Zero network attempts recorded; all five documents converted offline.
- Controls: the sandbox blocks a real HTTPS request while the same request works outside it; the recorder logs a planted connect; the same Docling call without the offline settings and seeded models tries to resolve `huggingface.co` and fails.
- Finding: a first version of the 4x4 PDF table (no caption, little other content on the page) was not detected as a table at all (0 of 16 cells; the text came out as line fragments). Later variants, including the same 4x4 table with a heading, were detected. Table detection depends on page layout and is not guaranteed.

## Design consequences
- Every number or table cell that a release cites is re-checked deterministically against the PDF text layer, not only against Docling's table output; a claim whose cited cell cannot be found by the second path is dropped or flagged.
- Source anchors for DOCX and HTML carry structure positions (paragraph, row, column), not pages, because those formats have no page.

## Not covered
Scanned PDFs and OCR; very large files and time/memory limits (parse of the five files took about 9 seconds including model load); password-protected and malformed files; hostile PDFs (hidden text, embedded scripts) are part of the later parser attack suite (SM tests); Linux sandbox equivalent (Seatbelt here is macOS only); comparison against the other parsers.
