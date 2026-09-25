//! Packaged local Lucene adapter. macOS development confinement only; release gate open.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub mod image;
pub mod image_regions;
pub mod ocr;
pub mod ocr_regions;
pub mod parser;
pub mod pdf_render;
pub(crate) mod python_graph;
pub(crate) mod search;
pub(crate) mod search_lifecycle;
#[cfg(target_os = "macos")]
mod supervision;

#[derive(Clone, Default)]
pub struct CancellationToken(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

#[derive(Clone)]
pub struct Runtime {
    pub root: PathBuf,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchHit {
    pub id: String,
    pub name: String,
    pub score: f64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchResults {
    pub workspace_revision: String,
    pub hits: Vec<SearchHit>,
    pub total: u64,
}

fn write_new(path: &std::path::Path, bytes: &[u8]) -> crate::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
