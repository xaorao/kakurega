import json
import os
import socket
import sys
from pathlib import Path

attempts = []
_orig_connect = socket.socket.connect
_orig_getaddrinfo = socket.getaddrinfo


def _record(kind, target):
    attempts.append(f"{kind}:{target}")


def patched_connect(self, address):
    if self.family != getattr(socket, "AF_UNIX", -1):
        _record("connect", address)
        raise OSError("network blocked by harness")
    return _orig_connect(self, address)


def patched_getaddrinfo(host, *a, **k):
    _record("dns", host)
    raise OSError("dns blocked by harness")


socket.socket.connect = patched_connect
socket.getaddrinfo = patched_getaddrinfo

from docling.document_converter import DocumentConverter, PdfFormatOption
from docling.datamodel.base_models import InputFormat
from docling.datamodel.pipeline_options import PdfPipelineOptions

src = Path(sys.argv[1])
artifacts = sys.argv[2]
opts = PdfPipelineOptions(artifacts_path=artifacts, do_ocr=False, do_table_structure=True)
conv = DocumentConverter(format_options={InputFormat.PDF: PdfFormatOption(pipeline_options=opts)})
result = {}
for p in sorted(src.iterdir()):
    if p.name == "truth.json":
        continue
    res = conv.convert(str(p))
    doc = res.document
    items = []
    for t in doc.texts:
        prov = t.prov[0] if t.prov else None
        items.append({"kind": "text", "text": t.text, "page": prov.page_no if prov else None})
    for tb in doc.tables:
        prov = tb.prov[0] if tb.prov else None
        for cell in tb.data.table_cells:
            items.append({"kind": "cell", "text": cell.text, "row": cell.start_row_offset_idx, "col": cell.start_col_offset_idx,
                          "page": prov.page_no if prov else None, "table_ref": tb.self_ref})
    result[p.name] = {"status": str(res.status), "items": items}
print(json.dumps({"network_attempts": attempts, "docs": result}))
