import json
import sys
from pathlib import Path

from docx import Document
from openpyxl import Workbook
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import getSampleStyleSheet
from reportlab.platypus import PageBreak, Paragraph, SimpleDocTemplate, Spacer, Table, TableStyle
from reportlab.lib import colors

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
truth = []
styles = getSampleStyleSheet()

S1 = "The audit found that session tokens remain valid after logout in the payments service."
S3 = "Remediation requires revoking the token server side before the response is returned."
SMALL = [["Finding", "Severity", "File", "Line"],
         ["Token reuse", "High", "auth/session.rs", "88"],
         ["Verbose error", "Low", "api/errors.rs", "17"],
         ["Weak default", "Medium", "config/defaults.rs", "42"]]
TABLE = [["Finding", "Severity", "File", "Line"]] + [
    ["Issue %d" % i, ["High", "Low", "Medium"][i % 3], "src/module_%d/handler.rs" % i, str(10 + i * 7)] for i in range(1, 11)]

doc = SimpleDocTemplate(str(out / "report.pdf"), pagesize=A4)
story = [Paragraph("Security Review: Payments", styles["Title"]), Paragraph(S1, styles["BodyText"]), PageBreak()]

def grid(data, widths=None):
    t = Table(data, colWidths=widths)
    t.setStyle(TableStyle([("GRID", (0, 0), (-1, -1), 0.5, colors.black), ("BACKGROUND", (0, 0), (-1, 0), colors.lightgrey),
                           ("FONTSIZE", (0, 0), (-1, -1), 11), ("TOPPADDING", (0, 0), (-1, -1), 6), ("BOTTOMPADDING", (0, 0), (-1, -1), 6)]))
    return t


story += [Paragraph("Findings", styles["Heading1"]), Paragraph("Table 1. Findings by module", styles["Heading2"]), Spacer(1, 8),
          grid(TABLE, (110, 90, 200, 60)), PageBreak(),
          Paragraph("Remediation", styles["Heading1"]), Paragraph(S3, styles["BodyText"])]
doc.build(story)
truth.append({"doc": "report.pdf", "kind": "text", "text": S1, "page": 1})
for r, row in enumerate(TABLE):
    for c, cell in enumerate(row):
        truth.append({"doc": "report.pdf", "kind": "cell", "text": cell, "page": 2, "row": r, "col": c})
truth.append({"doc": "report.pdf", "kind": "text", "text": S3, "page": 3})
SmallDoc = SimpleDocTemplate(str(out / "small_table.pdf"), pagesize=A4)
SmallDoc.build([Paragraph("Quick list", styles["Heading1"]), Spacer(1, 12), grid(SMALL)])
for r, row in enumerate(SMALL):
    for c, cell in enumerate(row):
        truth.append({"doc": "small_table.pdf", "kind": "cell", "text": cell, "page": 1, "row": r, "col": c})

d = Document()
d.add_heading("Services Agreement", 1)
d.add_paragraph("The supplier shall deliver the review within thirty days of signature.")
tb = d.add_table(rows=3, cols=2)
rows = [["Party", "Role"], ["Acme Corp", "Customer"], ["Northwind Ltd", "Supplier"]]
for r, row in enumerate(rows):
    for c, v in enumerate(row):
        tb.cell(r, c).text = v
d.save(out / "contract.docx")
truth.append({"doc": "contract.docx", "kind": "text", "text": "The supplier shall deliver the review within thirty days of signature."})
for r, row in enumerate(rows):
    for c, cell in enumerate(row):
        truth.append({"doc": "contract.docx", "kind": "cell", "text": cell, "row": r, "col": c})

wb = Workbook()
ws = wb.active
ws.title = "Q1"
q1 = [["Region", "Revenue", "Cost"], ["North", 1200, 800], ["South", 950, 700], ["West", 1430, 910]]
for row in q1:
    ws.append(row)
ws2 = wb.create_sheet("Q2")
q2 = [["Region", "Revenue", "Cost"], ["North", 1310, 820], ["South", 990, 715]]
for row in q2:
    ws2.append(row)
wb.save(out / "figures.xlsx")
for sheet, data in (("Q1", q1), ("Q2", q2)):
    for r, row in enumerate(data):
        for c, cell in enumerate(row):
            truth.append({"doc": "figures.xlsx", "kind": "cell", "text": str(cell), "sheet": sheet, "row": r, "col": c})

(out / "page.html").write_text(
    "<html><body><h1>Release notes</h1><p>Version 2.4 fixes the logout bug.</p>"
    "<table><tr><th>Component</th><th>Change</th></tr><tr><td>auth</td><td>revoke on logout</td></tr></table></body></html>"
)
truth.append({"doc": "page.html", "kind": "text", "text": "Version 2.4 fixes the logout bug."})
for r, row in enumerate([["Component", "Change"], ["auth", "revoke on logout"]]):
    for c, cell in enumerate(row):
        truth.append({"doc": "page.html", "kind": "cell", "text": cell, "row": r, "col": c})

(out / "truth.json").write_text(json.dumps(truth, indent=1))
print("documents:", sorted(p.name for p in out.iterdir() if p.name != "truth.json"), "truth items:", len(truth))
