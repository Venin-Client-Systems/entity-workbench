//! Packaged local Lucene adapter. macOS development confinement only; release gate open.
#[cfg(target_os = "macos")]
use crate::policy::{WorkerLimits, WorkerRequest};
use crate::{domain::Evidence, policy::WorkerOperation, require, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[cfg(target_os = "macos")]
mod supervision;
use uuid::Uuid;
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
impl Runtime {
    pub fn search(
        &self,
        cache: &Path,
        revision: u64,
        evidence: &[Evidence],
        query: &str,
    ) -> Result<SearchResults> {
        require(
            !query.trim().is_empty() && query.len() <= 1024,
            "Query must contain 1 to 1024 bytes",
        )?;
        fs::create_dir_all(cache)?;
        let cache = cache.canonicalize()?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(cache.join("coordinator.lock"))?;
        lock.try_lock()
            .map_err(|_| Error::Blocked("Local index is already in use".into()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cache, fs::Permissions::from_mode(0o700))?;
        }
        let marker = cache
            .parent()
            .ok_or_else(|| Error::Validation("Invalid index directory".into()))?
            .join("lucene-revision.txt");
        let indexed = fs::read_to_string(&marker)
            .ok()
            .and_then(|s| s.parse::<u64>().ok());
        if indexed != Some(revision) {
            // A failed rebuild cannot leave a revision marker claiming a valid index.
            if marker.exists() {
                fs::remove_file(&marker)?;
            }
            let manifest = serde_json::to_vec(
                &serde_json::json!({"workspace_revision":revision,"documents":evidence.iter().filter_map(|e|e.text.as_ref().map(|text|serde_json::json!({"id":e.id,"name":e.name,"text":text}))).collect::<Vec<_>>()}),
            )?;
            require(
                manifest.len() <= 16 * 1024 * 1024,
                "Index manifest exceeds the development limit",
            )?;
            let input = format!("manifest-{}.json", Uuid::new_v4());
            write_new(&cache.join(&input), &manifest)?;
            let indexed_result = self.run(&cache, WorkerOperation::Index, &input);
            fs::remove_file(cache.join(input))?;
            indexed_result?;
            fs::write(&marker, revision.to_string())?;
        }
        let input = format!("query-{}.json", Uuid::new_v4());
        write_new(
            &cache.join(&input),
            &serde_json::to_vec(&serde_json::json!({"query":query}))?,
        )?;
        let bytes = self.run(&cache, WorkerOperation::Search, &input);
        fs::remove_file(cache.join(input))?;
        let result: SearchResults = serde_json::from_slice(&bytes?)?;
        require(
            result.workspace_revision == revision.to_string() && result.hits.len() <= 100,
            "Index revision or hit limit is invalid",
        )?;
        require(
            result
                .hits
                .iter()
                .all(|hit| hit.score.is_finite() && evidence.iter().any(|e| e.id == hit.id)),
            "Search result references unknown evidence",
        )?;
        Ok(result)
    }
    #[cfg(not(target_os = "macos"))]
    fn run(&self, _job: &Path, _operation: WorkerOperation, _input: &str) -> Result<Vec<u8>> {
        Err(Error::Blocked(
            "Native worker confinement is not verified on this platform".into(),
        ))
    }
    #[cfg(target_os = "macos")]
    fn run(&self, cache: &Path, operation: WorkerOperation, input: &str) -> Result<Vec<u8>> {
        let job = tempfile::Builder::new().prefix("job-").tempdir_in(cache)?;
        let job_path = job.path().canonicalize()?;
        let index = cache.join("index");
        fs::create_dir_all(&index)?;
        supervision::validate_index(&index)?;
        // Rust stages only this request's input. No worker can read sibling jobs.
        write_new(&job_path.join("input.json"), &fs::read(cache.join(input))?)?;
        let request = WorkerRequest {
            protocol_version: 1,
            job_id: Uuid::new_v4().to_string(),
            operation,
            inputs: vec!["input.json".into()],
            output: "result.json".into(),
            limits: WorkerLimits {
                seconds: 30,
                output_bytes: 1024 * 1024,
                pages: 10000,
                pixels: 1,
                archive_members: 0,
                archive_depth: 0,
                expanded_bytes: 16 * 1024 * 1024,
            },
        };
        let bytes = serde_json::to_vec(&request)?;
        crate::policy::validate_worker_request(&bytes)?;
        write_new(&job_path.join("request.json"), &bytes)?;
        supervision::run_java(
            &self.root,
            &job_path,
            &index,
            matches!(request.operation, WorkerOperation::Index),
            "workbench.SearchWorker",
            &[],
            std::time::Duration::from_secs(u64::from(request.limits.seconds)),
        )?;
        supervision::read_result(&job_path.join("result.json"), request.limits.output_bytes)
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
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
