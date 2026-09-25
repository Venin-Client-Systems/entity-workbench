//! Disposable document extraction; Rust validates derivatives, never worker writes.
use super::{CancellationToken, Runtime};
use crate::{require, Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
#[cfg(any(windows, test))]
mod windows;

pub const MAX_TEXT_BYTES: usize = 512_000;
pub const MAX_RESULT_BYTES: u64 = 2 * 1024 * 1024;
pub const LOCAL_FONT_PDF_PARSER: &str = "pdfbox-3.0.8-local-fonts-v1";
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseStatus {
    Complete,
    Partial,
    Unsupported,
    Failed,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseLimitation {
    NoSourceAnchors,
    EmbeddedDocumentsExcluded,
    OcrNotPerformed,
    TextLimit,
    MetadataLimit,
    PageLimit,
    FontSubstituted,
    FontCoverageUnverified,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseFailure {
    MalformedDocument,
    EncryptedDocument,
    ArchiveLimits,
    TextExtractionRestricted,
    FontAssetUnavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParseResult {
    pub protocol_version: u32,
    pub job_id: String,
    pub content_sha256: String,
    pub source_bytes: u64,
    pub parser: String,
    pub media_type: String,
    pub status: ParseStatus,
    pub text: String,
    #[serde(deserialize_with = "deserialize_metadata")]
    pub metadata: BTreeMap<String, Vec<String>>,
    pub limitations: Vec<ParseLimitation>,
    pub error: Option<ParseFailure>,
}

fn deserialize_metadata<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, Vec<String>>, D::Error> {
    use serde::de::{Error as _, MapAccess, Visitor};
    struct Metadata;
    impl<'de> Visitor<'de> for Metadata {
        type Value = BTreeMap<String, Vec<String>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("extraction metadata with unique keys")
        }

        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some(key) = map.next_key::<String>()? {
                match result.entry(key) {
                    std::collections::btree_map::Entry::Occupied(_) => {
                        return Err(A::Error::custom("Duplicate extraction metadata key"));
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(map.next_value::<Vec<String>>()?);
                    }
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Metadata)
}

/// Reused by canonical acceptance. A valid worker result is still unreviewed text.
pub fn validate_result(
    result: &ParseResult,
    expected_sha: &str,
    expected_bytes: u64,
) -> Result<()> {
    require(
        result.protocol_version == 1,
        "Unsupported extraction protocol",
    )?;
    uuid::Uuid::parse_str(&result.job_id)
        .map_err(|_| Error::Validation("Invalid extraction job ID".into()))?;
    require(
        result.content_sha256.len() == 64
            && result
                .content_sha256
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            && result.content_sha256 == expected_sha
            && result.source_bytes == expected_bytes
            && expected_bytes <= crate::policy::MAX_IMPORT_BYTES as u64,
        "Extraction is not bound to the assigned original",
    )?;
    require(
        result.text.len() <= MAX_TEXT_BYTES
            && result.text.chars().count() <= 128_000
            && !result.text.contains('\0'),
        "Extracted text exceeds policy",
    )?;
    let local_fonts = result.parser == LOCAL_FONT_PDF_PARSER;
    let pdf = result.parser == "pdfbox-3.0.8" || local_fonts;
    require(
        result.metadata.len() <= 32 && result.limitations.len() <= if local_fonts { 8 } else { 6 },
        "Extraction field count exceeds policy",
    )?;
    let mut bytes = 0usize;
    for (key, values) in &result.metadata {
        require(
            !key.is_empty()
                && key.len() <= 128
                && !key.chars().any(char::is_control)
                && values.len() <= 8,
            "Invalid extraction metadata",
        )?;
        bytes += key.len();
        for value in values {
            require(
                value.len() <= 4096 && !value.contains('\0'),
                "Metadata value exceeds policy",
            )?;
            bytes += value.len();
        }
    }
    require(bytes <= 32_768, "Extraction metadata exceeds total limit")?;
    require(
        result
            .limitations
            .iter()
            .enumerate()
            .all(|(i, item)| !result.limitations[..i].contains(item)),
        "Duplicate extraction limitation",
    )?;
    let known = match result.parser.as_str() {
        "utf8-v1" => result.media_type == "text/plain",
        "pdfbox-3.0.8" | LOCAL_FONT_PDF_PARSER => result.media_type == "application/pdf",
        "tika-ooxml-3.3.2" => {
            result.media_type
                == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        "zip-preflight-v1" => result.media_type == "application/zip",
        "unsupported-v1" => matches!(
            result.media_type.as_str(),
            "application/octet-stream" | "application/zip"
        ),
        _ => false,
    };
    require(known, "Unsupported parser identity or media type")?;
    match result.status {
        ParseStatus::Complete => require(
            result.parser == "utf8-v1"
                && result.error.is_none()
                && result.limitations == [ParseLimitation::NoSourceAnchors],
            "Invalid complete extraction claim",
        )?,
        ParseStatus::Partial => require(
            matches!(
                result.parser.as_str(),
                "utf8-v1" | "pdfbox-3.0.8" | LOCAL_FONT_PDF_PARSER | "tika-ooxml-3.3.2"
            ) && result.error.is_none()
                && !result.limitations.is_empty(),
            "Partial extraction requires limitations",
        )?,
        ParseStatus::Unsupported => require(
            result.parser == "unsupported-v1"
                && result.error.is_none()
                && result.text.is_empty()
                && result.metadata.is_empty(),
            "Unsupported format cannot contain extracted claims",
        )?,
        ParseStatus::Failed => require(
            result.parser != "unsupported-v1"
                && result.error.is_some()
                && result.text.is_empty()
                && result.metadata.is_empty(),
            "Failed extraction must retain an error without extracted claims",
        )?,
    }
    require(
        result
            .limitations
            .contains(&ParseLimitation::NoSourceAnchors),
        "Source anchor limitation is missing",
    )?;
    let allowed_limitations: &[ParseLimitation] = match result.parser.as_str() {
        "utf8-v1" => &[ParseLimitation::NoSourceAnchors, ParseLimitation::TextLimit],
        "pdfbox-3.0.8" | LOCAL_FONT_PDF_PARSER => &[
            ParseLimitation::NoSourceAnchors,
            ParseLimitation::EmbeddedDocumentsExcluded,
            ParseLimitation::OcrNotPerformed,
            ParseLimitation::TextLimit,
            ParseLimitation::MetadataLimit,
            ParseLimitation::PageLimit,
        ],
        "tika-ooxml-3.3.2" => &[
            ParseLimitation::NoSourceAnchors,
            ParseLimitation::EmbeddedDocumentsExcluded,
            ParseLimitation::TextLimit,
            ParseLimitation::MetadataLimit,
        ],
        _ => &[ParseLimitation::NoSourceAnchors],
    };
    require(
        result.limitations.iter().all(|item| {
            allowed_limitations.contains(item)
                || (local_fonts
                    && matches!(
                        item,
                        ParseLimitation::FontSubstituted | ParseLimitation::FontCoverageUnverified
                    ))
        }),
        "Limitations do not match the parser capability",
    )?;
    require(
        result
            .limitations
            .contains(&ParseLimitation::FontSubstituted)
            == result
                .limitations
                .contains(&ParseLimitation::FontCoverageUnverified),
        "Font substitution and unverified coverage must be disclosed together",
    )?;
    if let Some(error) = &result.error {
        let permitted = match result.parser.as_str() {
            "pdfbox-3.0.8" | LOCAL_FONT_PDF_PARSER => {
                matches!(
                    error,
                    ParseFailure::MalformedDocument
                        | ParseFailure::EncryptedDocument
                        | ParseFailure::TextExtractionRestricted
                ) || (local_fonts && *error == ParseFailure::FontAssetUnavailable)
            }
            "tika-ooxml-3.3.2" => matches!(error, ParseFailure::MalformedDocument),
            "zip-preflight-v1" => matches!(
                error,
                ParseFailure::MalformedDocument | ParseFailure::ArchiveLimits
            ),
            _ => false,
        };
        require(permitted, "Failure does not match the parser capability")?;
    }
    if result.parser == "utf8-v1" {
        require(
            result.metadata.is_empty(),
            "UTF-8 adapter does not publish metadata",
        )?;
        if result.status == ParseStatus::Partial {
            require(
                result.limitations.contains(&ParseLimitation::TextLimit),
                "UTF-8 truncation must be disclosed",
            )?;
        }
    }
    if pdf && result.status == ParseStatus::Partial {
        require(
            [
                ParseLimitation::NoSourceAnchors,
                ParseLimitation::OcrNotPerformed,
                ParseLimitation::EmbeddedDocumentsExcluded,
            ]
            .iter()
            .all(|item| result.limitations.contains(item)),
            "PDF limitations are incomplete",
        )?;
    }
    if result.parser == "tika-ooxml-3.3.2" && result.status == ParseStatus::Partial {
        require(
            [
                ParseLimitation::NoSourceAnchors,
                ParseLimitation::EmbeddedDocumentsExcluded,
            ]
            .iter()
            .all(|item| result.limitations.contains(item)),
            "DOCX limitations are incomplete",
        )?;
    }
    Ok(())
}

impl Runtime {
    pub fn parse(&self, scratch_root: &Path, original_bytes: &[u8]) -> Result<ParseResult> {
        self.parse_with_cancel(scratch_root, original_bytes, &CancellationToken::default())
    }
    pub fn parse_with_cancel(
        &self,
        scratch_root: &Path,
        original_bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ParseResult> {
        require(
            original_bytes.len() <= crate::policy::MAX_IMPORT_BYTES,
            "Original exceeds parser input limit",
        )?;
        if cancellation.is_cancelled() {
            return Err(Error::Blocked("Document parsing cancelled".into()));
        }
        self.parse_confined(scratch_root, original_bytes, cancellation)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn parse_confined(
        &self,
        _scratch_root: &Path,
        _bytes: &[u8],
        _cancellation: &CancellationToken,
    ) -> Result<ParseResult> {
        Err(Error::Blocked(
            "Document worker confinement is not verified on this platform".into(),
        ))
    }
    #[cfg(target_os = "windows")]
    fn parse_confined(
        &self,
        scratch_root: &Path,
        original_bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ParseResult> {
        windows::parse(&self.root, scratch_root, original_bytes, cancellation)
    }
    #[cfg(target_os = "macos")]
    fn parse_confined(
        &self,
        scratch_root: &Path,
        original_bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ParseResult> {
        use super::supervision;
        use crate::policy::{WorkerLimits, WorkerOperation, WorkerRequest};
        use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
        fs::create_dir_all(scratch_root)?;
        let scratch_root = scratch_root.canonicalize()?;
        fs::set_permissions(&scratch_root, fs::Permissions::from_mode(0o700))?;
        let job = tempfile::Builder::new()
            .prefix("parse-")
            .tempdir_in(&scratch_root)?;
        let outcome = (|| {
            let path = job.path().canonicalize()?;
            super::write_new(&path.join("input.json"), original_bytes)?;
            let request = WorkerRequest {
                protocol_version: 1,
                job_id: uuid::Uuid::new_v4().to_string(),
                operation: WorkerOperation::Parse,
                inputs: vec!["input.json".into()],
                output: "result.json".into(),
                limits: WorkerLimits {
                    seconds: 30,
                    output_bytes: MAX_RESULT_BYTES,
                    pages: 100,
                    pixels: 1,
                    archive_members: 1000,
                    archive_depth: 0,
                    expanded_bytes: 64 * 1024 * 1024,
                },
            };
            let bytes = serde_json::to_vec(&request)?;
            crate::policy::validate_worker_request(&bytes)?;
            super::write_new(&path.join("request.json"), &bytes)?;
            supervision::run_parser_java(
                &self.root,
                &path,
                "workbench.ParseWorker",
                &[],
                Duration::from_secs(30),
                cancellation,
            )?;
            let bytes = supervision::read_result(&path.join("result.json"), MAX_RESULT_BYTES)?;
            let result: ParseResult = serde_json::from_slice(&bytes)?;
            require(
                result.job_id == request.job_id,
                "Extraction belongs to another job",
            )?;
            validate_result(
                &result,
                &format!("{:x}", Sha256::digest(original_bytes)),
                original_bytes.len() as u64,
            )?;
            if cancellation.is_cancelled() {
                return Err(Error::Blocked("Document parsing cancelled".into()));
            }
            Ok(result)
        })();
        supervision::finish_job(job, outcome)
    }
}

#[cfg(test)]
mod tests;
