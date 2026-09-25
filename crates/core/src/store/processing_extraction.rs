//! Versioned parse inspection binds one immutable derivative to its original and owning job.
use super::*;
use crate::engines::parser::{LOCAL_FONT_PDF_PARSER, MAX_RESULT_BYTES};

fn bounded<T: DeserializeOwned>(conn: &Connection, kind: &str, key: &str) -> Result<T> {
    let bytes: Option<u64> = conn
        .query_row(
            "SELECT length(CAST(body AS BLOB)) FROM records WHERE kind=? AND id=?",
            params![kind, key],
            |row| row.get(0),
        )
        .optional()?;
    let bytes = bytes.ok_or_else(|| Error::Validation(format!("Unknown {kind} identifier")))?;
    require(
        bytes > 0 && bytes <= MAX_RESULT_BYTES,
        "Extraction or owning job exceeds the retained record limit",
    )?;
    // The caller owns a SQLite snapshot, so the checked body cannot change before this read.
    get(conn, kind, key)
}

pub(super) fn validate_record(record: &ExtractionRecord, expected_id: &str) -> Result<()> {
    require(
        matches!(record.schema_version, 1 | 2)
            && record.id == expected_id
            && record.attempt > 0
            && record.attempt <= MAX_ATTEMPTS
            && record.id == hash(format!("{}:{}", record.job_id, record.attempt).as_bytes()),
        "Invalid extraction version or identity",
    )?;
    let owner = uuid::Uuid::parse_str(&record.job_id)
        .map_err(|_| Error::Validation("Invalid extraction owner".into()))?;
    require(
        owner.to_string() == record.job_id,
        "Extraction owner UUID is not canonical",
    )?;
    require(
        record.created_at.len() <= 64
            && chrono::DateTime::parse_from_rfc3339(&record.created_at).is_ok(),
        "Invalid extraction publication timestamp",
    )?;
    require(
        record.schema_version == 2 || record.result.parser != LOCAL_FONT_PDF_PARSER,
        "Extraction v1 cannot contain the local-font parser contract",
    )?;
    let ParseDocumentInput::ParseDocument {
        evidence_id,
        sha256,
        bytes,
    } = &record.input;
    require(evidence_id == sha256, "Extraction source identity changed")?;
    validate_result(&record.result, sha256, *bytes)?;
    require(
        record.result_sha256 == hash(&serde_json::to_vec(&record.result)?),
        "Extraction result digest changed",
    )?;
    require(
        serde_json::to_vec(record)?.len() as u64 <= MAX_RESULT_BYTES,
        "Extraction record exceeds the retained record limit",
    )?;
    Ok(())
}

impl Workspace {
    pub fn extraction(&self, extraction_id: &str) -> Result<ExtractionRecord> {
        require(
            extraction_id.len() == 64
                && extraction_id
                    .bytes()
                    .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase()),
            "Invalid extraction identifier",
        )?;
        let snapshot = self.conn.unchecked_transaction()?;
        let record: ExtractionRecord = bounded(&snapshot, "extraction", extraction_id)?;
        validate_record(&record, extraction_id)?;
        let ParseDocumentInput::ParseDocument {
            evidence_id,
            sha256,
            bytes,
        } = &record.input;
        let input = ProcessingInput::ParseDocument {
            evidence_id: evidence_id.clone(),
            sha256: sha256.clone(),
            bytes: *bytes,
        };
        let job: ProcessingJob = bounded(&snapshot, "processing_job", &record.job_id)?;
        supported_job(&job)?;
        require(
            job.id == record.job_id
                && job.input == input
                && job.attempt >= record.attempt
                && job.attempt <= MAX_ATTEMPTS
                && job.result_ids.contains(&record.id),
            "Extraction is not owned by this processing attempt",
        )?;
        self.verify_processing_input(&input)?;
        snapshot.commit()?;
        Ok(record)
    }
}

#[cfg(test)]
#[path = "processing_extraction_tests.rs"]
mod tests;
