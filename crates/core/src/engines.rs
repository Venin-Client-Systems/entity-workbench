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
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};
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
            let manifest = serde_json::to_vec(
                &serde_json::json!({"workspace_revision":revision,"documents":evidence.iter().filter_map(|e|e.text.as_ref().map(|text|serde_json::json!({"id":e.id,"name":e.name,"text":text}))).collect::<Vec<_>>()}),
            )?;
            require(
                manifest.len() <= 16 * 1024 * 1024,
                "Index manifest exceeds the development limit",
            )?;
            let input = format!("manifest-{}.json", Uuid::new_v4());
            write_new(&cache.join(&input), &manifest)?;
            self.run(&cache, WorkerOperation::Index, &input)?;
            fs::remove_file(cache.join(input))?;
            fs::write(&marker, revision.to_string())?;
        }
        let input = format!("query-{}.json", Uuid::new_v4());
        write_new(
            &cache.join(&input),
            &serde_json::to_vec(&serde_json::json!({"query":query}))?,
        )?;
        let result: SearchResults =
            serde_json::from_slice(&self.run(&cache, WorkerOperation::Search, &input)?)?;
        fs::remove_file(cache.join(input))?;
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
    fn run(&self, job: &Path, operation: WorkerOperation, input: &str) -> Result<Vec<u8>> {
        let root = self
            .root
            .canonicalize()
            .map_err(|_| Error::Blocked("Packaged Java runtime is unavailable".into()))?;
        let java = root.join("java/bin/java");
        require(java.is_file(), "Packaged Java runtime is missing")?;
        let key = Uuid::new_v4().to_string();
        let output = format!("result-{key}.json");
        let request = WorkerRequest {
            protocol_version: 1,
            job_id: key,
            operation,
            inputs: vec![input.into()],
            output: output.clone(),
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
        let quote =
            |path: &Path| serde_json::to_string(&path.to_string_lossy()).map_err(Error::from);
        let profile=format!("(version 1)\n(deny default)\n(import \"dyld-support.sb\")\n(allow process-fork)\n(allow sysctl-read)\n(allow file-read-metadata)\n(allow process-exec (literal {}))\n(allow file-read* file-map-executable (subpath {}) (subpath \"/usr/lib\") (subpath \"/System\"))\n(allow file-read* (literal \"/dev/random\") (literal \"/dev/urandom\") (literal \"/dev/null\") (subpath {}))\n(allow file-write* (subpath {}))",quote(&java)?,quote(&root)?,quote(job)?,quote(job)?);
        let profile_path = job.join(format!("worker-{}.sb", Uuid::new_v4()));
        write_new(&profile_path, profile.as_bytes())?;
        let request_path = job.join(format!("request-{}.json", Uuid::new_v4()));
        write_new(&request_path, &bytes)?;
        let classpath = format!(
            "{}:{}",
            root.join("search/workers-0.1.0.jar").display(),
            root.join("search/lib/*").display()
        );
        let mut child = Command::new("/usr/bin/sandbox-exec")
            .args(["-f"])
            .arg(&profile_path)
            .arg(java)
            .args(["-Xmx256m", "-XX:-UsePerfData"])
            .arg(format!("-Djava.io.tmpdir={}", job.display()))
            .args(["-cp", &classpath, "workbench.SearchWorker"])
            .current_dir(job)
            .env_clear()
            // Preserve the actual OS home value: sandbox-exec on macOS 26 crashes
            // when it is absent. No other caller environment is inherited.
            .envs(std::env::vars_os().filter(|(key, _)| key == "HOME"))
            .stdin(fs::File::open(request_path)?)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > Duration::from_secs(30) {
                child.kill()?;
                child.wait()?;
                return Err(Error::Blocked("Local search worker timed out".into()));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        require(
            status.success(),
            "Local search worker failed; no result was accepted",
        )?;
        let path = job.join(output);
        let metadata = fs::symlink_metadata(&path)?;
        require(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= 1024 * 1024,
            "Worker returned an invalid result file",
        )?;
        let result = fs::read(&path)?;
        fs::remove_file(path)?;
        Ok(result)
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
