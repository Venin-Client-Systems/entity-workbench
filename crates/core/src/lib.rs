//! Canonical workspace ownership, domain rules and reviewed engine coordination.
pub mod analytics;
pub mod collection;
pub mod collection_receipt;
pub mod domain;
pub mod engines;
pub mod policy;
pub mod report;
pub mod statements;
pub mod store;

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
    #[error("Network request failed: {0}")]
    Network(String),
    #[error("Database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Worker cleanup failed: {0}")]
    Cleanup(String),
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
