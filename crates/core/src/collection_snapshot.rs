//! Lossless local v4 acquisition snapshots, not legacy receipts or benchmark results.
use crate::{collection_api::CollectionRunInspection, domain::Acquisition};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
pub const ORIGINAL_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSource {
    pub evidence_id: String,
    pub sha256: String,
    pub bytes: u64,
    /// Digest of source metadata at capture; unrelated metadata is not exported.
    pub canonical_metadata_sha256: String,
    /// Only acquisition identities for this exact run; duplicates are preserved.
    pub acquisitions: Vec<Acquisition>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DurableCollectionSnapshot {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub canonical_run_sha256: String,
    pub canonical_run_bytes: u64,
    /// Exact UTF-8 SQLite body. It is retained as inert JSON, including lease/history.
    pub canonical_run_json: String,
    pub inspection: CollectionRunInspection,
    pub sources: Vec<SnapshotSource>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CollectionExportFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DurableCollectionExport {
    pub schema_version: u32,
    pub export_id: String,
    pub job_id: String,
    pub workspace_revision: u64,
    pub canonical_run_sha256: String,
    pub snapshot: CollectionExportFile,
    pub originals: Vec<CollectionExportFile>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DurableCollectionExportInspection {
    pub receipt: DurableCollectionExport,
    pub snapshot: DurableCollectionSnapshot,
}
