//! Canonical workspace ownership, domain rules and reviewed engine coordination.
pub mod analytics;
pub mod citation_catalogue;
pub mod collection;
pub mod collection_api;
pub(crate) mod collection_execution;
pub(crate) mod collection_jobs;
pub(crate) mod collection_machine;
pub mod collection_receipt;
pub(crate) mod collection_settlement;
pub(crate) mod collection_transport;
pub mod coordinator;
pub mod desktop_summary;
pub mod docx_snapshot;
pub mod domain;
pub mod engines;
pub mod literal_search;
pub mod local_export;
pub mod policy;
pub mod processing;
pub mod report;
pub mod report_document;
pub mod report_docx;
pub mod review_decision_page;
pub mod statements;
pub mod store;
pub mod transaction_analysis;
pub mod transaction_balance;
pub mod transaction_comparison;
pub mod transaction_export;
pub mod transaction_facets;
pub mod transaction_page;
pub mod transaction_search;
pub mod transaction_sources;
pub mod transfer_candidates;

use thiserror::Error;
#[derive(Debug, Error)]
pub enum Error {
    #[error("Validation: {0}")]
    Validation(String),
    #[error("Workspace conflict: {0}")]
    Conflict(String),
    #[error("Capability blocked: {0}")]
    Blocked(String),
    #[error("Collection limit exhausted: {0}")]
    QuotaExhausted(String),
    #[error("Processing interrupted: {0}")]
    Interrupted(String),
    #[error("Network request failed: {0}")]
    Network(String),
    #[error("Database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Worker cleanup failed: {0}")]
    Cleanup(String),
    #[error("Worker termination could not be verified: {0}")]
    TerminationUnverified(String),
    #[error("Worker result was rejected: {0}")]
    InvalidWorkerResult(String),
    #[error("Derivative storage is unavailable (published reference: {published})")]
    DerivativeUnavailable { published: bool },
    #[error("Filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid CSV: {0}")]
    Csv(#[from] csv::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Validation(message.into()))
    }
}
