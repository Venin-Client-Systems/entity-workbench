"""Generate small deterministic synthetic parser inputs; no external assets."""
from pathlib import Path
import hashlib
import json
import zipfile

ROOT = Path(__file__).resolve().parents[1] / "fixtures/parser"
ROOT.mkdir(parents=True, exist_ok=True)
(ROOT / "notice.txt").write_text("SYNTHETIC DOCUMENT\nRowan Ellis owns Fictional Harbour Cooperative.\nReference DEMO-000017.\n", encoding="utf-8")


def pdf(path, content):
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream",
    ]
    data = b"%PDF-1.4\n%synthetic\n"
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data += str(index).encode() + b" 0 obj\n" + obj + b"\nendobj\n"
    xref = len(data)
    data += b"xref\n0 6\n0000000000 65535 f \n"
    data += b"".join(f"{offset:010} 00000 n \n".encode() for offset in offsets[1:])
    data += f"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    path.write_bytes(data)


pdf(ROOT / "notice.pdf", b"BT /F1 12 Tf 48 730 Td (SYNTHETIC DOCUMENT - Rowan Ellis - Fictional Harbour Cooperative) Tj ET")
pdf(ROOT / "no-text.pdf", b"0.7 g 48 680 300 40 re f")
parts = {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
    "_rels/.rels": '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/document.xml": '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>SYNTHETIC DOCUMENT - Rowan Ellis - Fictional Harbour Cooperative</w:t></w:r></w:p></w:body></w:document>',
}
with zipfile.ZipFile(ROOT / "notice.docx", "w", compression=zipfile.ZIP_DEFLATED) as package:
    for name, content in parts.items():
        info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        package.writestr(info, content)
with zipfile.ZipFile(ROOT / "traversal.zip", "w") as package:
    package.writestr(zipfile.ZipInfo("../outside.txt", date_time=(2026, 1, 1, 0, 0, 0)), "synthetic")
(ROOT / "manifest.json").write_text(json.dumps({"schema_version": 1, "synthetic": True, "files": [{"path": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "size": path.stat().st_size} for path in sorted(ROOT.iterdir()) if path.name != "manifest.json"]}, indent=2) + "\n")
