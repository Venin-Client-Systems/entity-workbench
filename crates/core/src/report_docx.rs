//! Fixed editable WordprocessingML package. No templates, external resources or HTML conversion.
use crate::{
    domain::SourceAnchor,
    report_document::{CappedWriter, ReportDocument},
    require, Error, Result,
};
use quick_xml::{
    events::{BytesDecl, BytesEnd, BytesRef, BytesStart, BytesText, Event},
    Writer,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Cursor, Seek, SeekFrom, Write},
};
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, System, ZipWriter};

pub const MAX_DOCX_BYTES: usize = 32 * 1024 * 1024;
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const PACKAGE_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>";
const PACKAGE_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";
const DOCUMENT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>";

pub fn render(document: &ReportDocument) -> Result<Vec<u8>> {
    document.validate()?;
    let mut xml = DocumentWriter::new(document)?;
    xml.body(document)?;
    let body = xml.xml.finish();
    let styles = styles()?;
    let core = properties(document)?;
    let mut archive = ZipWriter::new(BoundedArchive::new(MAX_DOCX_BYTES));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .system(System::Unix)
        .unix_permissions(0o600);
    // Fixed names and order; nothing in the input becomes a path or relationship target.
    for (name, bytes) in [
        ("[Content_Types].xml", PACKAGE_TYPES.as_bytes()),
        ("_rels/.rels", PACKAGE_RELS.as_bytes()),
        ("docProps/core.xml", core.as_slice()),
        ("word/document.xml", body.as_slice()),
        ("word/styles.xml", styles.as_slice()),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS.as_bytes()),
    ] {
        archive.start_file(name, options).map_err(zip_error)?;
        archive.write_all(bytes)?;
    }
    Ok(archive.finish().map_err(zip_error)?.cursor.into_inner())
}
fn zip_error(error: zip::result::ZipError) -> Error {
    Error::Validation(format!("DOCX package could not be completed: {error}"))
}
struct Xml {
    writer: Writer<CappedWriter<Vec<u8>>>,
}
impl Xml {
    fn new() -> Result<Self> {
        let mut writer = Writer::new(CappedWriter::new(Vec::new(), MAX_DOCX_BYTES));
        writer.write_event(Event::Decl(BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            Some("yes"),
        )))?;
        Ok(Self { writer })
    }
    fn start(&mut self, name: &str, attrs: &[(&str, &str)]) -> Result<()> {
        let mut tag = BytesStart::new(name);
        for (key, value) in attrs {
            tag.push_attribute((*key, *value));
        }
        self.writer.write_event(Event::Start(tag))?;
        Ok(())
    }
    fn end(&mut self, name: &str) -> Result<()> {
        self.writer.write_event(Event::End(BytesEnd::new(name)))?;
        Ok(())
    }
    fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) -> Result<()> {
        let mut tag = BytesStart::new(name);
        for (key, value) in attrs {
            tag.push_attribute((*key, *value));
        }
        self.writer.write_event(Event::Empty(tag))?;
        Ok(())
    }
    fn literal(&mut self, value: &str) -> Result<()> {
        self.writer
            .write_event(Event::Text(BytesText::new(value)))?;
        Ok(())
    }
    fn text_element(&mut self, name: &str, value: &str) -> Result<()> {
        self.start(name, &[])?;
        self.literal(value)?;
        self.end(name)
    }
    fn run(&mut self, value: &str) -> Result<()> {
        self.start("w:r", &[])?;
        let mut begin = 0;
        for (index, c) in value.char_indices() {
            if matches!(c, '\n' | '\r' | '\t') {
                self.word_text(&value[begin..index])?;
                match c {
                    '\n' => self.empty("w:br", &[])?,
                    '\t' => self.empty("w:tab", &[])?,
                    _ => {
                        self.start("w:t", &[("xml:space", "preserve")])?;
                        self.writer
                            .write_event(Event::GeneralRef(BytesRef::new("#13")))?;
                        self.end("w:t")?;
                    }
                }
                begin = index + c.len_utf8();
            }
        }
        self.word_text(&value[begin..])?;
        self.end("w:r")
    }
    fn word_text(&mut self, value: &str) -> Result<()> {
        if value.is_empty() {
            return Ok(());
        }
        self.start("w:t", &[("xml:space", "preserve")])?;
        self.literal(value)?;
        self.end("w:t")
    }
    fn finish(self) -> Vec<u8> {
        self.writer.into_inner().inner
    }
}
struct DocumentWriter {
    xml: Xml,
    bookmarks: BTreeMap<String, (usize, String)>,
}
impl DocumentWriter {
    fn new(d: &ReportDocument) -> Result<Self> {
        let c = &d.content;
        let ids = c
            .hypotheses
            .iter()
            .map(|x| &x.id)
            .chain(c.findings.iter().map(|x| &x.id))
            .chain(c.transactions.iter().map(|x| &x.id))
            .chain(c.entities.iter().map(|x| &x.id))
            .chain(c.observations.iter().map(|x| &x.id))
            .chain(c.evidence.iter().map(|x| &x.id))
            .chain(c.identity_decisions.iter().map(|x| &x.id))
            .chain(c.merges.iter().map(|x| &x.id))
            .chain(c.decisions.iter().map(|x| &x.id));
        let mut names = BTreeSet::new();
        let mut bookmarks = BTreeMap::new();
        for (number, id) in ids.enumerate() {
            // Word bookmark names stay below 40 ASCII characters. A collision is an error.
            let name = format!("ew_{:x}", Sha256::digest(id.as_bytes()));
            let name = name[..35].to_string();
            require(names.insert(name.clone()), "Report bookmark collision")?;
            require(
                bookmarks.insert(id.clone(), (number, name)).is_none(),
                "Duplicate report bookmark",
            )?;
        }
        Ok(Self {
            xml: Xml::new()?,
            bookmarks,
        })
    }
    fn paragraph(&mut self, style: &str, text: &str, bookmark: Option<&str>) -> Result<()> {
        self.xml.start("w:p", &[])?;
        self.xml.start("w:pPr", &[])?;
        self.xml.empty("w:pStyle", &[("w:val", style)])?;
        if text.len() <= 2048 && text.lines().count() <= 10 {
            self.xml.empty("w:keepLines", &[])?;
        }
        self.xml.end("w:pPr")?;
        if let Some(id) = bookmark {
            let (number, name) = self
                .bookmarks
                .get(id)
                .ok_or_else(|| Error::Validation("Missing bookmark".into()))?;
            self.xml.empty(
                "w:bookmarkStart",
                &[("w:id", &number.to_string()), ("w:name", name)],
            )?;
        }
        self.xml.run(text)?;
        if let Some(id) = bookmark {
            self.xml.empty(
                "w:bookmarkEnd",
                &[("w:id", &self.bookmarks[id].0.to_string())],
            )?;
        }
        self.xml.end("w:p")
    }
    fn links(&mut self, label: &str, ids: &[String]) -> Result<()> {
        self.links_with_next(label, ids, false)
    }
    fn links_with_next(&mut self, label: &str, ids: &[String], keep_next: bool) -> Result<()> {
        self.xml.start("w:p", &[])?;
        if keep_next {
            self.xml.start("w:pPr", &[])?;
            self.xml.empty("w:keepNext", &[])?;
            self.xml.end("w:pPr")?;
        }
        self.xml.run(&format!("{label} ({})", ids.len()))?;
        for id in ids {
            let (number, name) = self
                .bookmarks
                .get(id)
                .ok_or_else(|| Error::Validation("Dangling report link".into()))?;
            self.xml.run(" · ")?;
            self.xml.start("w:hyperlink", &[("w:anchor", name)])?;
            self.xml.run(&format!("Record {}", number + 1))?;
            self.xml.end("w:hyperlink")?;
        }
        self.xml.end("w:p")
    }
    fn heading(&mut self, title: &str) -> Result<()> {
        self.paragraph("Heading1", title, None)
    }
    fn record(&mut self, title: &str, id: &str) -> Result<()> {
        self.paragraph("Heading2", title, Some(id))?;
        self.paragraph(
            "FollowingMetadata",
            &format!("Record {} · ID {id}", self.bookmarks[id].0 + 1),
            None,
        )
    }
    fn table_start(&mut self, widths: &[u32]) -> Result<()> {
        self.xml.start("w:tbl", &[])?;
        self.xml.start("w:tblPr", &[])?;
        self.xml
            .empty("w:tblW", &[("w:w", "9360"), ("w:type", "dxa")])?;
        self.xml.empty("w:tblLayout", &[("w:type", "fixed")])?;
        self.xml.start("w:tblBorders", &[])?;
        for border in ["top", "left", "bottom", "right", "insideH", "insideV"] {
            self.xml.empty(
                &format!("w:{border}"),
                &[("w:val", "single"), ("w:sz", "4"), ("w:color", "D9D9D9")],
            )?;
        }
        self.xml.end("w:tblBorders")?;
        self.xml.start("w:tblCellMar", &[])?;
        for side in ["top", "left", "bottom", "right"] {
            self.xml
                .empty(&format!("w:{side}"), &[("w:w", "90"), ("w:type", "dxa")])?;
        }
        self.xml.end("w:tblCellMar")?;
        self.xml.end("w:tblPr")?;
        self.xml.start("w:tblGrid", &[])?;
        for width in widths {
            self.xml
                .empty("w:gridCol", &[("w:w", &width.to_string())])?;
        }
        self.xml.end("w:tblGrid")
    }
    fn row(
        &mut self,
        values: &[&str],
        widths: &[u32],
        header: bool,
        bookmark: Option<&str>,
    ) -> Result<()> {
        self.xml.start("w:tr", &[])?;
        self.xml.start("w:trPr", &[])?;
        if header {
            self.xml.empty("w:tblHeader", &[])?;
        }
        // Keep modest rows together; long records can still flow across pages.
        if values.iter().map(|v| v.len()).sum::<usize>() <= 2048
            && values.iter().all(|v| v.lines().count() <= 10)
        {
            self.xml.empty("w:cantSplit", &[])?;
        }
        self.xml.end("w:trPr")?;
        for (index, (value, width)) in values.iter().zip(widths).enumerate() {
            self.xml.start("w:tc", &[])?;
            self.xml.start("w:tcPr", &[])?;
            self.xml
                .empty("w:tcW", &[("w:w", &width.to_string()), ("w:type", "dxa")])?;
            if header {
                self.xml.empty("w:shd", &[("w:fill", "E8ECE7")])?;
            }
            self.xml.empty("w:vAlign", &[("w:val", "center")])?;
            self.xml.end("w:tcPr")?;
            self.paragraph(
                if header { "TableHeader" } else { "Small" },
                value,
                if index == 0 { bookmark } else { None },
            )?;
            self.xml.end("w:tc")?;
        }
        self.xml.end("w:tr")
    }
    fn body(&mut self, d: &ReportDocument) -> Result<()> {
        let c = &d.content;
        self.xml.start("w:document", &[("xmlns:w", W)])?;
        self.xml.start("w:body", &[])?;
        self.paragraph("Title", "Investigation assessment", None)?;
        self.paragraph(
            "Normal",
            &format!(
                "Snapshot {} · Workspace revision {}\nCreated {}",
                d.report_id, d.workspace_revision, d.created_at
            ),
            None,
        )?;
        self.paragraph("Normal", "This assessment retains the findings, opposing evidence and calculation inputs at the stated revision. Review states and limitations govern interpretation. Pending records are not accepted conclusions. Source origin groups do not establish independence.", None)?;
        self.paragraph(
            "Small",
            &format!(
                "Template {} · Generator {}",
                d.template_version, d.generator_version
            ),
            None,
        )?;
        self.heading("Questions and alternatives")?;
        for h in &c.hypotheses {
            self.record(&h.question, &h.id)?;
            self.paragraph("Normal", &h.proposition, None)?;
            for a in &h.alternatives {
                self.paragraph("Normal", &format!("Alternative: {a}"), None)?;
            }
            for gap in &h.gaps {
                self.paragraph("Normal", &format!("Collection gap: {gap}"), None)?;
            }
        }
        self.heading("Findings")?;
        for f in &c.findings {
            self.record(&f.title, &f.id)?;
            self.paragraph("Normal", &f.assessment, None)?;
            self.paragraph(
                "Normal",
                &format!(
                    "Review required: {}\nLimitations: {}",
                    f.needs_review, f.limitations
                ),
                None,
            )?;
            self.links("Supporting evidence", &f.supporting_ids)?;
            self.links("Contradicting evidence", &f.contradicting_ids)?;
            self.links("Linked questions", &f.hypothesis_ids)?;
        }
        self.heading("Reviewed transaction calculations")?;
        self.paragraph("Normal", "Only accepted records contribute. Explicitly matched transfers are excluded by the shared analytical engine. No currency conversion is performed. Record links refer to the frozen versions below.", None)?;
        for t in &d.calculations.totals {
            self.paragraph("Heading2", &t.currency, None)?;
            self.paragraph(
                "Normal",
                &format!(
                    "Credits {} − debits {} = net {}",
                    t.credits, t.debits, t.net
                ),
                None,
            )?;
            self.links("Included accepted records", &t.transaction_ids)?;
            self.links("Excluded matched transfers", &t.excluded_transfer_ids)?;
        }
        let a = &d.calculations;
        self.paragraph("Normal", &format!("Whole-ledger review denominator: {} transactions. Accepted {} (including matched transfers); pending {}; rejected {}; deferred {}.", c.transactions.len(), a.accepted, a.pending, a.rejected, a.deferred), None)?;
        if a.totals.is_empty() {
            self.paragraph(
                "Normal",
                "No accepted transaction totals are available.",
                None,
            )?;
        }
        self.paragraph("Normal", "Balance checks use all intervening source rows within an imported account and currency, regardless of review state. Opening balances are not inferred.", None)?;
        for b in &a.balances {
            self.paragraph(
                "Normal",
                &format!(
                    "Balance difference {} · Reconciled {}",
                    b.difference, b.reconciled
                ),
                None,
            )?;
            self.links(
                "Previous and current balance records",
                &[b.previous_id.clone(), b.transaction_id.clone()],
            )?;
            self.links("Contributing source rows", &b.transaction_ids)?;
        }
        self.heading("Transaction register")?;
        let widths = [1350, 1000, 3210, 1600, 1150, 1050];
        self.table_start(&widths)?;
        self.row(
            &[
                "Date",
                "Account",
                "Original description",
                "Amount",
                "Review",
                "Version",
            ],
            &widths,
            true,
            None,
        )?;
        for t in &c.transactions {
            self.row(
                &[
                    &t.date,
                    &t.account,
                    &t.description,
                    &format!("{} {}", t.amount, t.currency),
                    &format!("{:?}", t.review),
                    &t.version.to_string(),
                ],
                &widths,
                false,
                Some(&t.id),
            )?;
        }
        self.xml.end("w:tbl")?;
        self.heading("Transaction provenance")?;
        for t in &c.transactions {
            self.links_with_next("Transaction", std::slice::from_ref(&t.id), true)?;
            self.links_with_next(
                "Retained source",
                &[t.anchor.evidence_id().to_owned()],
                true,
            )?;
            self.paragraph(
                "FollowingMetadata",
                &format!(
                    "ID {} · Version {}\nPosting date: {} · Available balance: {}\n{}",
                    t.id,
                    t.version,
                    t.posting_date.as_deref().unwrap_or("Not provided"),
                    t.balance.as_deref().unwrap_or("Not provided"),
                    anchor_label(&t.anchor)
                ),
                None,
            )?;
            if let Some(merchant) = &t.merchant {
                self.paragraph("Normal", &format!("Merchant: {merchant}"), None)?;
            }
            self.links_with_next(
                "Duplicate candidates",
                &t.duplicate_candidates,
                t.transfer_peer.is_some(),
            )?;
            if let Some(peer) = &t.transfer_peer {
                self.links("Transfer peer", std::slice::from_ref(peer))?;
            }
        }
        self.heading("Entity register")?;
        for e in &c.entities {
            self.record(&e.name, &e.id)?;
            self.paragraph("Normal", &format!("Kind: {:?}", e.kind), None)?;
            for i in &e.identifiers {
                self.paragraph(
                    "Normal",
                    &format!("Identifier namespace: {}\nValue: {}", i.namespace, i.value),
                    None,
                )?;
            }
            if let Some(target) = &e.merged_into {
                self.links("Merged into", std::slice::from_ref(target))?;
            }
        }
        self.heading("Identity decisions")?;
        if c.identity_decisions.is_empty() && c.merges.is_empty() {
            self.paragraph(
                "Normal",
                "No identity or merge decisions are retained.",
                None,
            )?;
        }
        for decision in &c.identity_decisions {
            self.record("Identity decision", &decision.id)?;
            self.links(
                "Compared entities",
                &[decision.left_id.clone(), decision.right_id.clone()],
            )?;
            self.paragraph(
                "Normal",
                &format!(
                    "{:?} · {}\n{}",
                    decision.outcome, decision.at, decision.reason
                ),
                None,
            )?;
        }
        for m in &c.merges {
            self.record("Merge decision", &m.id)?;
            self.links("Source and target", &[m.source.clone(), m.target.clone()])?;
            self.paragraph(
                "Normal",
                &format!("Reversed: {}\n{}", m.reversed, m.reason),
                None,
            )?;
        }
        self.heading("Review history")?;
        for decision in &c.decisions {
            self.record("Review decision", &decision.id)?;
            self.paragraph(
                "Normal",
                &format!(
                    "Target {} · {:?} · {}\n{}",
                    decision.target_id, decision.state, decision.at, decision.reason
                ),
                None,
            )?;
        }
        self.heading("Observations")?;
        for o in &c.observations {
            self.record(&o.field, &o.id)?;
            self.paragraph("Normal", &o.value, None)?;
            self.paragraph(
                "Normal",
                &format!(
                    "Review: {:?} · Extraction quality: {}",
                    o.review,
                    o.extraction_quality
                        .map_or_else(|| "Not provided".into(), |q| q.to_string())
                ),
                None,
            )?;
            self.links("Entity", std::slice::from_ref(&o.entity_id))?;
            self.links_with_next(
                "Retained source",
                &[o.anchor.evidence_id().to_owned()],
                true,
            )?;
            self.paragraph("Small", &anchor_label(&o.anchor), None)?;
        }
        self.heading("Evidence register")?;
        for e in &c.evidence {
            self.record(&e.name, &e.id)?;
            self.paragraph("Small", &format!("SHA-256 {}\nBytes {} · Media type {}\nOrigin group {} · Imported {}\nExtraction {}", e.sha256, e.bytes, e.media_type, e.origin_group, e.imported_at, e.extraction_status), None)?;
            self.paragraph(
                "Normal",
                e.text.as_deref().unwrap_or(
                    "No retained text derivative is available. Consult the original evidence.",
                ),
                None,
            )?;
            for acquisition in &e.acquisitions {
                self.paragraph(
                    "Normal",
                    &format!(
                        "Acquisition URL: {}\nRetrieved {} · Job {}",
                        acquisition.url, acquisition.retrieved_at, acquisition.job_id
                    ),
                    None,
                )?;
            }
        }
        self.heading("Limitations and outstanding enquiries")?;
        self.paragraph("Normal", "No automated identity conclusion or physical-presence inference is made. Source independence, geographic coverage and extraction completeness require review. Citations preserve existing source anchors; document pagination creates no new evidence anchors. This editable export represents a frozen revision. Editing a copy does not modify the retained workspace.", None)?;
        self.xml.start("w:sectPr", &[])?;
        self.xml
            .empty("w:pgSz", &[("w:w", "12240"), ("w:h", "15840")])?;
        self.xml.empty(
            "w:pgMar",
            &[
                ("w:top", "1440"),
                ("w:right", "1440"),
                ("w:bottom", "1440"),
                ("w:left", "1440"),
                ("w:header", "0"),
                ("w:footer", "0"),
                ("w:gutter", "0"),
            ],
        )?;
        self.xml.end("w:sectPr")?;
        self.xml.end("w:body")?;
        self.xml.end("w:document")
    }
}
// The source bookmark immediately above each label binds this location to the
// full evidence digest. The frozen JSON retains the exact typed anchor unchanged.
fn anchor_label(anchor: &SourceAnchor) -> String {
    match anchor {
        SourceAnchor::Text {
            line_start,
            line_end,
            ..
        } => {
            format!("Text lines {line_start} to {line_end}")
        }
        SourceAnchor::Page { page, region, .. } => match region {
            Some([x, y, width, height]) => {
                format!("Page {page}\nRetained region [{x}, {y}, {width}, {height}]")
            }
            None => format!("Page {page} · No region retained"),
        },
        SourceAnchor::Cell {
            sheet, row, column, ..
        } => {
            format!("Sheet: {sheet}\nRow {row} · Column: {column}")
        }
        SourceAnchor::Message { message_id, .. } => format!("Message ID: {message_id}"),
        SourceAnchor::Capture { selector, .. } => format!("Capture selector: {selector}"),
    }
}

fn styles() -> Result<Vec<u8>> {
    let mut x = Xml::new()?;
    x.start("w:styles", &[("xmlns:w", W)])?;
    for (id, name, size, bold, next) in [
        ("Normal", "Normal", "22", false, false),
        ("Title", "Title", "40", true, true),
        ("Heading1", "heading 1", "28", true, true),
        ("Heading2", "heading 2", "23", true, true),
        ("Small", "Small", "18", false, false),
        ("FollowingMetadata", "Following Metadata", "18", false, true),
        ("TableHeader", "Table Header", "18", true, false),
    ] {
        x.start("w:style", &[("w:type", "paragraph"), ("w:styleId", id)])?;
        x.empty("w:name", &[("w:val", name)])?;
        if id != "Normal" {
            x.empty("w:basedOn", &[("w:val", "Normal")])?;
        }
        x.empty("w:next", &[("w:val", "Normal")])?;
        x.start("w:pPr", &[])?;
        if next {
            x.empty("w:keepNext", &[])?;
        }
        if id == "Heading1" {
            x.empty("w:outlineLvl", &[("w:val", "0")])?;
        }
        if id == "Heading2" {
            x.empty("w:outlineLvl", &[("w:val", "1")])?;
        }
        // Character-level wrapping prevents a long identifier or literal selector
        // from extending beyond the page without inserting characters into its text.
        x.empty("w:wordWrap", &[("w:val", "off")])?;
        x.empty(
            "w:spacing",
            &[
                ("w:before", if id == "Heading1" { "240" } else { "0" }),
                (
                    "w:after",
                    if matches!(id, "Small" | "FollowingMetadata") {
                        "70"
                    } else {
                        "140"
                    },
                ),
                ("w:line", "260"),
                ("w:lineRule", "auto"),
            ],
        )?;
        x.end("w:pPr")?;
        x.start("w:rPr", &[])?;
        x.empty(
            "w:rFonts",
            &[
                ("w:ascii", "Arial"),
                ("w:hAnsi", "Arial"),
                ("w:eastAsia", "Arial"),
                ("w:cs", "Arial"),
            ],
        )?;
        x.empty("w:color", &[("w:val", "000000")])?;
        x.empty("w:sz", &[("w:val", size)])?;
        if bold {
            x.empty("w:b", &[])?;
        }
        x.end("w:rPr")?;
        x.end("w:style")?;
    }
    x.end("w:styles")?;
    Ok(x.finish())
}
fn properties(d: &ReportDocument) -> Result<Vec<u8>> {
    let mut x = Xml::new()?;
    x.start(
        "cp:coreProperties",
        &[
            (
                "xmlns:cp",
                "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
            ),
            ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
        ],
    )?;
    x.text_element("dc:title", "Investigation assessment")?;
    x.text_element("dc:creator", "Entity Workbench")?;
    x.text_element("dc:identifier", &d.report_id)?;
    x.text_element(
        "dc:description",
        &format!(
            "Workspace revision {} · Created {} · {}",
            d.workspace_revision, d.created_at, d.generator_version
        ),
    )?;
    x.end("cp:coreProperties")?;
    Ok(x.finish())
}
struct BoundedArchive {
    cursor: Cursor<Vec<u8>>,
    limit: usize,
}
impl BoundedArchive {
    fn new(limit: usize) -> Self {
        Self {
            cursor: Cursor::new(Vec::new()),
            limit,
        }
    }
}
impl Write for BoundedArchive {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self
            .cursor
            .position()
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::other("DOCX size overflow"))?;
        if end > self.limit as u64 {
            return Err(io::Error::other("DOCX exceeds 32 MiB"));
        }
        self.cursor.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Seek for BoundedArchive {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let target = match position {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(n) => self.cursor.get_ref().len() as i128 + i128::from(n),
            SeekFrom::Current(n) => i128::from(self.cursor.position()) + i128::from(n),
        };
        if target < 0 || target > self.limit as i128 {
            return Err(io::Error::other("DOCX seek exceeds byte bounds"));
        }
        self.cursor.seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_cap_rejects_seek_growth_and_overwrite_expansion() {
        let mut file = BoundedArchive::new(4);
        file.write_all(b"1234").unwrap();
        assert!(file.write_all(b"5").is_err());
        assert!(file.seek(SeekFrom::Start(5)).is_err());
        assert!(file.seek(SeekFrom::End(-5)).is_err());
        file.seek(SeekFrom::Start(1)).unwrap();
        file.write_all(b"abc").unwrap();
        assert_eq!(file.cursor.into_inner(), b"1abc");
    }
}
