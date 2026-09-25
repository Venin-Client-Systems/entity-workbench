//! Immutable report objects precede one revision-guarded canonical publication.
use super::derivative_files::{self as files, ObjectRef};
use super::*;
use crate::{
    docx_snapshot::*,
    report_document::{self, ReportDocument},
    report_docx,
};
mod catalogue;

const KIND: &str = "docx_snapshot";
const MAX_RECORD_BYTES: u64 = 4096;
fn key(value: &str) -> Result<()> {
    require(
        value.len() == 36 && Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value),
        "Invalid DOCX snapshot request identity",
    )
}
fn validate(record: &DocxSnapshotRecord) -> Result<()> {
    key(&record.id)?;
    require(
        record.schema_version == 1
            && record.template_version == report_document::TEMPLATE_VERSION
            && report_document::supported_generator(&record.generator_version),
        "Unsupported DOCX snapshot format",
    )?;
    require(
        record.created_at.len() <= 64
            && chrono::DateTime::parse_from_rfc3339(&record.created_at).is_ok(),
        "Invalid DOCX snapshot time",
    )?;
    require(
        record.document.kind == ReportArtifactKind::ReportDocumentJsonV1
            && record.docx.kind == ReportArtifactKind::ReportDocxV1,
        "Incorrect DOCX artifact kinds",
    )?;
    record.document.validate()?;
    record.docx.validate()
}
fn lookup(conn: &Connection, id: &str) -> Result<Option<DocxSnapshotRecord>> {
    key(id)?;
    let size: Option<u64> = conn
        .query_row(
            "SELECT length(CAST(body AS BLOB)) FROM records WHERE kind=? AND id=?",
            params![KIND, id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(size) = size else { return Ok(None) };
    require(
        size <= MAX_RECORD_BYTES,
        "DOCX snapshot metadata exceeds its bound",
    )?;
    let record: DocxSnapshotRecord = get(conn, KIND, id)?;
    validate(&record)?;
    require(record.id == id, "DOCX snapshot key differs from its record")?;
    Ok(Some(record))
}
pub(super) fn refs(record: &DocxSnapshotRecord) -> [ObjectRef; 2] {
    [
        ObjectRef::Report(record.document.clone()),
        ObjectRef::Report(record.docx.clone()),
    ]
}
fn reference(kind: ReportArtifactKind, bytes: &[u8]) -> Result<ReportArtifactRef> {
    let reference = ReportArtifactRef {
        kind,
        sha256: hash(bytes),
        bytes: bytes.len() as u64,
    };
    reference.validate()?;
    Ok(reference)
}
impl Workspace {
    /// Recover one uncertain caller identity without scanning the catalogue or
    /// regenerating current content. Corrupt/unavailable records are errors.
    pub fn resolve_docx_capture(
        &self,
        request_id: &str,
        captured_revision: u64,
    ) -> Result<DocxCaptureResolution> {
        self.resolve_docx_capture_checked(request_id, captured_revision, || Ok(()))
    }
    fn resolve_docx_capture_checked(
        &self,
        request_id: &str,
        captured_revision: u64,
        after_revision: impl FnOnce() -> Result<()>,
    ) -> Result<DocxCaptureResolution> {
        key(request_id)?;
        let transaction = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision < captured_revision {
            return Err(Error::Conflict(
                "Workspace revision precedes the retained DOCX request".into(),
            ));
        }
        after_revision()?;
        let outcome = if let Some(record) = lookup(&transaction, request_id)? {
            require(
                record.workspace_revision == captured_revision,
                "DOCX request identity belongs to another source revision",
            )?;
            self.verify_docx_snapshot(&record)?;
            DocxCaptureOutcome::Saved {
                snapshot: Box::new(record),
            }
        } else {
            DocxCaptureOutcome::NotRecorded
        };
        transaction.commit()?;
        Ok(DocxCaptureResolution {
            schema_version: 1,
            request_id: request_id.into(),
            captured_revision,
            workspace_revision: revision,
            outcome,
        })
    }
    /// The caller retains one UUID through retries. This operation never modifies an existing snapshot.
    pub fn save_docx_snapshot(
        &mut self,
        request_id: &str,
        expected_revision: u64,
    ) -> Result<DocxSnapshotRecord> {
        self.save_docx_snapshot_checked(request_id, expected_revision, |_| Ok(()))
    }
    fn save_docx_snapshot_checked(
        &mut self,
        request_id: &str,
        expected_revision: u64,
        after_retain: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<DocxSnapshotRecord> {
        key(request_id)?;
        if let Some(record) = lookup(&self.conn, request_id)? {
            require(
                record.workspace_revision == expected_revision,
                "DOCX request identity was already used for another revision",
            )?;
            return Ok(self
                .inspect_docx_snapshot(&record.id, &record.document.sha256, &record.docx.sha256)?
                .snapshot);
        }
        require(
            self.conn
                .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?
                == 5,
            "DOCX publication requires workspace schema 5",
        )?;
        if self.revision()? != expected_revision {
            return Err(Error::Conflict(
                "Workspace changed before DOCX capture".into(),
            ));
        }
        let view = self.view_with_reports(|_| Ok(Vec::<ReportSnapshot>::new()))?;
        if view.revision != expected_revision {
            return Err(Error::Conflict(
                "Workspace changed before DOCX capture".into(),
            ));
        }
        let document = report_document::capture(&view, request_id, &now())?;
        for evidence in &document.content.evidence {
            self.verify_original(evidence)?;
        }
        let json = document.to_json()?;
        let docx = report_docx::render(&document)?;
        let record = DocxSnapshotRecord {
            schema_version: 1,
            id: request_id.into(),
            workspace_revision: expected_revision,
            created_at: document.created_at,
            template_version: document.template_version,
            generator_version: document.generator_version,
            document: reference(ReportArtifactKind::ReportDocumentJsonV1, &json)?,
            docx: reference(ReportArtifactKind::ReportDocxV1, &docx)?,
        };
        validate(&record)?;
        for (reference, bytes) in refs(&record).iter().zip([json.as_slice(), docx.as_slice()]) {
            let published: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM derivative_objects WHERE sha256=?)",
                [reference.sha256()],
                |r| r.get(0),
            )?;
            if published {
                files::verify_object_catalog(&self.conn, reference)?;
                files::read_object(&self.root, reference)?;
            } else {
                files::retain_object(&self.root, reference, bytes)?;
            }
        }
        after_retain(&self.root)?;
        for evidence in &document.content.evidence {
            self.verify_original(evidence)?;
        }
        // Revalidate retained objects after preparation before canonical references exist.
        for reference in refs(&record) {
            files::read_object(&self.root, &reference)?;
        }
        self.change(
            Some(expected_revision),
            "report.docx_snapshot",
            false,
            |conn| {
                require(
                    lookup(conn, request_id)?.is_none(),
                    "DOCX snapshot identity already exists",
                )?;
                let collision: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM records WHERE kind='report' AND id=?)",
                    [request_id],
                    |r| r.get(0),
                )?;
                require(
                    !collision,
                    "DOCX request identity collides with an HTML report",
                )?;
                for reference in refs(&record) {
                    files::catalog_object(conn, &reference)?;
                }
                put(conn, KIND, request_id, &record)
            },
        )?;
        Ok(record)
    }
    /// Explicit bounded typed read. Verification uses frozen content, never current analytical rows.
    pub fn inspect_docx_snapshot(
        &self,
        report_id: &str,
        expected_document_sha256: &str,
        expected_docx_sha256: &str,
    ) -> Result<DocxSnapshotInspection> {
        let transaction = self.conn.unchecked_transaction()?;
        let record = lookup(&transaction, report_id)?.ok_or_else(|| {
            Error::Blocked(
                "No retained DOCX snapshot; existing HTML-only reports cannot be regenerated"
                    .into(),
            )
        })?;
        require(
            record.document.sha256 == expected_document_sha256
                && record.docx.sha256 == expected_docx_sha256,
            "DOCX snapshot identity changed",
        )?;
        let (document, _) = self.verify_docx_snapshot(&record)?;
        transaction.commit()?;
        Ok(DocxSnapshotInspection {
            snapshot: record,
            document,
        })
    }
    pub fn read_docx_snapshot(
        &self,
        report_id: &str,
        expected_document_sha256: &str,
        expected_docx_sha256: &str,
    ) -> Result<Vec<u8>> {
        self.read_docx_snapshot_artifact(report_id, expected_document_sha256, expected_docx_sha256)
            .map(|(_, bytes)| bytes)
    }
    pub(crate) fn read_docx_snapshot_artifact(
        &self,
        report_id: &str,
        expected_document_sha256: &str,
        expected_docx_sha256: &str,
    ) -> Result<(DocxSnapshotRecord, Vec<u8>)> {
        let transaction = self.conn.unchecked_transaction()?;
        let record = lookup(&transaction, report_id)?
            .ok_or_else(|| Error::Blocked("No retained DOCX snapshot".into()))?;
        require(
            record.document.sha256 == expected_document_sha256
                && record.docx.sha256 == expected_docx_sha256,
            "DOCX snapshot identity changed",
        )?;
        let (_, bytes) = self.verify_docx_snapshot(&record)?;
        transaction.commit()?;
        Ok((record, bytes))
    }
    pub(super) fn docx_records(&self) -> Result<Vec<DocxSnapshotRecord>> {
        let mut statement = self
            .conn
            .prepare("SELECT CASE WHEN length(CAST(id AS BLOB))=36 THEN id ELSE NULL END FROM records WHERE kind=? ORDER BY sequence")?;
        let keys = statement.query_map([KIND], |r| r.get::<_, String>(0))?;
        keys.map(|item| {
            lookup(&self.conn, &item?)?
                .ok_or_else(|| Error::Validation("Missing DOCX record".into()))
        })
        .collect()
    }
    pub(super) fn verify_docx_snapshot(
        &self,
        record: &DocxSnapshotRecord,
    ) -> Result<(ReportDocument, Vec<u8>)> {
        validate(record)?;
        require(
            record.workspace_revision < self.revision()?,
            "DOCX snapshot source revision is not historical",
        )?;
        for reference in refs(record) {
            files::verify_object_catalog(&self.conn, &reference)?;
        }
        let json = files::read_object(&self.root, &ObjectRef::Report(record.document.clone()))?;
        let document = ReportDocument::from_json(&json)?;
        require(
            document.to_json()? == json
                && document.report_id == record.id
                && document.workspace_revision == record.workspace_revision
                && document.created_at == record.created_at
                && document.template_version == record.template_version
                && document.generator_version == record.generator_version,
            "Frozen DOCX document identity differs from its snapshot",
        )?;
        for evidence in &document.content.evidence {
            let canonical = get_evidence(&self.conn, &evidence.id)?;
            require(
                canonical.bytes == evidence.bytes,
                "Frozen report original length differs from canonical evidence",
            )?;
            self.verify_original(evidence)?;
        }
        let docx = files::read_object(&self.root, &ObjectRef::Report(record.docx.clone()))?;
        require(
            report_docx::render(&document)? == docx,
            "Retained DOCX differs from its frozen document",
        )?;
        Ok((document, docx))
    }
}
#[cfg(test)]
mod tests;
