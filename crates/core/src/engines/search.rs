//! Existing fixed Lucene recipe with explicit index/assignment ownership.
use super::search_corpus::SearchCorpus;
#[cfg(any(test, target_os = "macos"))]
use super::search_corpus::{validate_query, CorpusBuilder};
#[cfg(any(test, target_os = "macos"))]
use super::search_lifecycle::Lease;
#[cfg(target_os = "macos")]
use super::{search_lifecycle, supervision, write_new};
use super::{search_lifecycle::Completion, Runtime, SearchResults};
#[cfg(target_os = "macos")]
use crate::policy::{WorkerLimits, WorkerRequest};
use crate::{domain::Evidence, Error, Result};
#[cfg(any(test, target_os = "macos"))]
use crate::{policy::WorkerOperation, require};
#[cfg(any(test, target_os = "macos"))]
use serde::Deserialize;
#[cfg(any(test, target_os = "macos"))]
use std::fs;
use std::path::Path;
#[cfg(any(test, target_os = "macos"))]
use uuid::Uuid;

#[cfg(any(test, target_os = "macos"))]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexResults {
    indexed: u64,
    workspace_revision: u64,
}

#[cfg(any(test, target_os = "macos"))]
fn accept_index_result(bytes: &[u8], revision: u64, documents: u64, marker: &Path) -> Result<()> {
    let result: IndexResults = serde_json::from_slice(bytes)?;
    require(
        result.workspace_revision == revision && result.indexed == documents,
        "Index acknowledgement does not match the assigned revision and document count",
    )?;
    fs::write(marker, revision.to_string())?;
    Ok(())
}

impl Runtime {
    pub fn search(
        &self,
        cache: &Path,
        revision: u64,
        evidence: &[Evidence],
        query: &str,
    ) -> Result<SearchResults> {
        self.search_completed(cache, revision, evidence, query)
            .result
    }
    pub(crate) fn search_completed(
        &self,
        cache: &Path,
        revision: u64,
        evidence: &[Evidence],
        query: &str,
    ) -> Completion<SearchResults> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (cache, revision, evidence, query);
            Completion::released(Err(Error::Blocked(
                "Native worker confinement is not verified on this platform".into(),
            )))
        }
        #[cfg(target_os = "macos")]
        self.search_with(
            cache,
            revision,
            evidence,
            query,
            |cache, operation, input| self.run(cache, operation, input),
        )
    }
    pub(crate) fn search_corpus_completed(
        &self,
        cache: &Path,
        corpus: &SearchCorpus,
        query: &str,
    ) -> Completion<SearchResults> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (cache, corpus, query);
            Completion::released(Err(Error::Blocked(
                "Native worker confinement is not verified on this platform".into(),
            )))
        }
        #[cfg(target_os = "macos")]
        self.search_corpus_with(cache, corpus, query, |cache, operation, input| {
            self.run(cache, operation, input)
        })
    }
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn search_with(
        &self,
        cache: &Path,
        revision: u64,
        evidence: &[Evidence],
        query: &str,
        execute: impl FnMut(&Path, WorkerOperation, &str) -> Result<Vec<u8>>,
    ) -> Completion<SearchResults> {
        let prepare = || {
            validate_query(query)?;
            let mut builder = CorpusBuilder::new(revision)?;
            for row in evidence {
                builder.push(row)?;
            }
            builder.finish()
        };
        match prepare() {
            Ok(corpus) => self.search_corpus_with(cache, &corpus, query, execute),
            Err(error) => Completion::released(Err(error)),
        }
    }
    #[cfg(any(test, target_os = "macos"))]
    pub(crate) fn search_corpus_with(
        &self,
        cache: &Path,
        corpus: &SearchCorpus,
        query: &str,
        mut execute: impl FnMut(&Path, WorkerOperation, &str) -> Result<Vec<u8>>,
    ) -> Completion<SearchResults> {
        if let Err(error) = validate_query(query) {
            return Completion::released(Err(error));
        }
        let revision = corpus.revision();
        let mut lease = match Lease::acquire(cache) {
            Ok(lease) => lease,
            Err(failure) => {
                return Completion {
                    result: Err(failure.result.expect_err("failed acquisition")),
                    disposition: failure.disposition,
                }
            }
        };
        let result = (|| {
            lease.prepare(revision)?;
            let cache = lease.cache().to_owned();
            let marker = cache
                .parent()
                .ok_or_else(|| Error::Validation("Invalid index directory".into()))?
                .join("lucene-revision.txt");
            let indexed = fs::read_to_string(&marker)
                .ok()
                .and_then(|value| value.parse::<u64>().ok());
            #[cfg(target_os = "macos")]
            let indexed =
                indexed.filter(|_| supervision::validate_index(&cache.join("index")).is_ok());
            if indexed != Some(revision) {
                if marker.exists() {
                    fs::remove_file(&marker)?;
                }
                let input = format!("manifest-{}.json", Uuid::new_v4());
                lease.stage(&input, corpus.manifest())?;
                let bytes = execute(&cache, WorkerOperation::Index, &input)?;
                accept_index_result(&bytes, revision, corpus.document_count(), &marker)?;
            }
            let input = format!("query-{}.json", Uuid::new_v4());
            lease.stage(
                &input,
                &serde_json::to_vec(&serde_json::json!({"query":query}))?,
            )?;
            let bytes = execute(&cache, WorkerOperation::Search, &input)?;
            let result: SearchResults = serde_json::from_slice(&bytes)?;
            require(
                result.workspace_revision == revision.to_string() && result.hits.len() <= 100,
                "Index revision or hit limit is invalid",
            )?;
            require(
                result
                    .hits
                    .iter()
                    .all(|hit| hit.score.is_finite() && corpus.knows(&hit.id)),
                "Search result references unknown evidence",
            )?;
            Ok(result)
        })();
        lease.finish(result)
    }
    #[cfg(target_os = "macos")]
    fn run(&self, cache: &Path, operation: WorkerOperation, input: &str) -> Result<Vec<u8>> {
        #[cfg(test)]
        probe::enter(&operation)?;
        with_assignment(cache, |job| {
            let job_path = job.canonicalize()?;
            let index = cache.join("index");
            if matches!(operation, WorkerOperation::Index) {
                // A rebuild never reuses a worker-controlled failed derivative.
                supervision::cleanup_tree(&index)?;
                fs::create_dir(&index)?;
            }
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
        })
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "search_probe.rs"]
pub(crate) mod probe;

#[cfg(target_os = "macos")]
fn with_assignment(
    cache: &Path,
    execute: impl FnOnce(&Path) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    // Keep immediately: unwinding cannot invoke TempDir's unverified implicit cleanup.
    let job = tempfile::Builder::new()
        .prefix("job-")
        .tempdir_in(cache)?
        .keep();
    let result = execute(&job);
    if matches!(result, Err(Error::TerminationUnverified(_))) {
        return result;
    }
    match supervision::cleanup_tree(&job) {
        Ok(()) => result,
        Err(error) => Err(search_lifecycle::cleanup_error(result, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_acknowledgement_is_required_before_revision_publication() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("revision");
        for response in [
            b"".as_slice(),
            b"{}",
            b"not-json",
            b"{\"indexed\":1,\"workspace_revision\":6}",
            b"{\"indexed\":2,\"workspace_revision\":7}",
            b"{\"indexed\":1,\"workspace_revision\":7,\"extra\":true}",
            b"{\"indexed\":1,\"workspace_revision\":7} {}",
            b"{\"indexed\":\"1\",\"workspace_revision\":7}",
        ] {
            // Even an otherwise successful worker exit cannot publish these bytes.
            assert!(accept_index_result(response, 7, 1, &marker).is_err());
            assert!(!marker.exists());
        }
        accept_index_result(b"{\"indexed\":1,\"workspace_revision\":7}", 7, 1, &marker).unwrap();
        assert_eq!(fs::read_to_string(marker).unwrap(), "7");
    }
}

#[cfg(all(test, target_os = "macos"))]
mod assignment_tests {
    use super::*;
    #[test]
    fn search_lifecycle_actual_assignment_wrapper_retains_unknown_and_panic() {
        for panic in [false, true] {
            let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
            let returned = std::panic::catch_unwind(|| {
                with_assignment(root.path(), |job| {
                    fs::write(job.join("sentinel"), b"retained")?;
                    if panic {
                        panic!("synthetic wrapper panic");
                    }
                    Err(Error::TerminationUnverified("exact unknown".into()))
                })
            });
            if panic {
                assert!(returned.is_err());
            } else {
                assert!(
                    matches!(returned.unwrap(), Err(Error::TerminationUnverified(ref value)) if value == "exact unknown")
                );
            }
            let children: Vec<_> = fs::read_dir(root.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            assert_eq!(children.len(), 1);
            assert_eq!(fs::read(children[0].join("sentinel")).unwrap(), b"retained");
        }
    }
    #[test]
    fn search_lifecycle_actual_assignment_wrapper_cleans_only_known_stop() {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let error = with_assignment(root.path(), |job| {
            fs::write(job.join("input"), b"owned")?;
            Err(Error::QuotaExhausted("exact quota".into()))
        });
        assert!(matches!(error, Err(Error::QuotaExhausted(ref value)) if value == "exact quota"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
