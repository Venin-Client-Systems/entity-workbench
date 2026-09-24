//! Fixed development recipes. No application adapter calls these yet.
// Native collection/acceptance is intentionally inactive on other platforms.
#![cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) mod diagnostics;
#[cfg(windows)]
mod probe;
mod runtime;
use crate::{Error, Request, Result};
#[cfg(windows)]
pub use probe::development_probe;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, time::Duration};
use uuid::Uuid;

pub(crate) const INDEX_BYTES: usize = 24 * 1024 * 1024;
pub(crate) const INDEX_FILE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const INDEX_MEMBERS: usize = 128;
pub(crate) const PARSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Parser,
    Search,
}
#[derive(Debug, Clone, Serialize)]
pub struct Document {
    pub id: String,
    pub name: String,
    pub text: String,
}
#[derive(Clone)]
pub(crate) struct IndexFile {
    pub(crate) name: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) sha256: String,
}
/// Only a successfully validated build can construct a snapshot. It is owned
/// data, never an existing directory or permission grant supplied by a caller.
#[derive(Clone)]
pub struct IndexSnapshot {
    pub(crate) revision: u64,
    documents: BTreeMap<String, String>,
    pub(crate) files: Vec<IndexFile>,
}
impl IndexSnapshot {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
    pub fn total_bytes(&self) -> usize {
        self.files.iter().map(|f| f.bytes.len()).sum()
    }
}
enum Operation {
    Parse,
    Index {
        revision: u64,
        documents: BTreeMap<String, String>,
    },
    Search {
        snapshot: IndexSnapshot,
    },
}
pub struct Job {
    id: Uuid,
    operation: Operation,
    input: Vec<u8>,
}
fn bounded(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Blocked(message))
    }
}
const INPUT_BYTES: usize = 16 * 1024 * 1024;
struct BoundedJson(Vec<u8>);
impl std::io::Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let size = self
            .0
            .len()
            .checked_add(bytes.len())
            .filter(|size| *size <= INPUT_BYTES)
            .ok_or_else(|| std::io::Error::other("fixed JSON byte bound exceeded"))?;
        if size > self.0.capacity() {
            let capacity = self
                .0
                .capacity()
                .max(4096)
                .saturating_mul(2)
                .max(size)
                .min(INPUT_BYTES);
            self.0
                .try_reserve_exact(capacity - self.0.len())
                .map_err(std::io::Error::other)?;
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Job {
    pub fn parse(original: Vec<u8>) -> Result<Self> {
        bounded(
            original.len() <= 16 * 1024 * 1024,
            "parser input exceeds bound",
        )?;
        Ok(Self {
            id: Uuid::new_v4(),
            operation: Operation::Parse,
            input: original,
        })
    }
    pub fn index(revision: u64, documents: Vec<Document>) -> Result<Self> {
        bounded(
            documents.len() <= 10000,
            "index document count exceeds bound",
        )?;
        // Bound borrowed source strings before cloning identities or serializing.
        // Escaping can expand JSON, so a separate capped writer also bounds output.
        let mut source_bytes = 0usize;
        for document in &documents {
            for value in [&document.id, &document.name, &document.text] {
                source_bytes = source_bytes
                    .checked_add(value.len())
                    .ok_or(Error::Blocked("index input size overflow"))?;
                bounded(
                    source_bytes <= INPUT_BYTES,
                    "index source strings exceed bound",
                )?;
            }
        }
        let mut ids = BTreeMap::new();
        for document in &documents {
            bounded(
                !document.id.is_empty()
                    && document.id.len() <= 128
                    && !document.id.chars().any(char::is_control)
                    && document.name.len() <= 1024
                    && !document.name.contains('\0')
                    && ids
                        .insert(document.id.clone(), document.name.clone())
                        .is_none(),
                "invalid or duplicate index document identity",
            )?;
        }
        #[derive(Serialize)]
        struct Manifest<'a> {
            workspace_revision: u64,
            documents: &'a [Document],
        }
        let mut writer = BoundedJson(Vec::new());
        serde_json::to_writer(
            &mut writer,
            &Manifest {
                workspace_revision: revision,
                documents: &documents,
            },
        )
        .map_err(|_| Error::Blocked("index JSON exceeds encoding bound"))?;
        let input = writer.0;
        Ok(Self {
            id: Uuid::new_v4(),
            operation: Operation::Index {
                revision,
                documents: ids,
            },
            input,
        })
    }
    pub fn search(snapshot: &IndexSnapshot, query: &str) -> Result<Self> {
        bounded(
            !query.trim().is_empty() && query.len() <= 1024,
            "search query exceeds bound",
        )?;
        validate_snapshot(snapshot)?;
        Ok(Self {
            id: Uuid::new_v4(),
            operation: Operation::Search {
                snapshot: snapshot.clone(),
            },
            input: serde_json::to_vec(&serde_json::json!({"query":query}))
                .map_err(|_| Error::Blocked("search input encoding failed"))?,
        })
    }
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn role(&self) -> Role {
        if matches!(self.operation, Operation::Parse) {
            Role::Parser
        } else {
            Role::Search
        }
    }
    fn operation_name(&self) -> &'static str {
        match self.operation {
            Operation::Parse => "parse",
            Operation::Index { .. } => "index",
            Operation::Search { .. } => "search",
        }
    }
}
pub struct JavaOutput {
    pub job_id: Uuid,
    /// Bounded, strictly decoded transport result; canonical acceptance remains
    /// the core coordinator's responsibility, including all source-anchor rules.
    pub bytes: Vec<u8>,
    pub output_sha256: String,
    pub index: Option<IndexSnapshot>,
}
pub(crate) struct Prepared<'a> {
    pub(crate) request: Request,
    pub(crate) metadata: Vec<u8>,
    pub(crate) job: &'a Job,
}
impl Prepared<'_> {
    pub(crate) fn build_index(&self) -> bool {
        matches!(self.job.operation, Operation::Index { .. })
    }
    pub(crate) fn snapshot(&self) -> Option<&IndexSnapshot> {
        match &self.job.operation {
            Operation::Search { snapshot } => Some(snapshot),
            _ => None,
        }
    }
    pub(crate) fn output_limit(&self) -> u64 {
        if self.job.role() == Role::Parser {
            PARSE_BYTES
        } else {
            1024 * 1024
        }
    }
    pub(crate) fn verify_runtime(&self, path: &Path) -> Result<()> {
        runtime::verify(path, self.job.role())
    }
}
pub(crate) fn prepare<'a>(
    root: &Path,
    scratch_parent: &Path,
    job: &'a Job,
) -> Result<Prepared<'a>> {
    // Substitution can produce verbatim Windows paths. Appended separators must
    // already be backslashes; Win32 does not normalize '/' after a \\?\ prefix.
    let mut arguments = vec![
        "-Xmx256m".into(),
        "-XX:-UsePerfData".into(),
        "-XX:+DisableAttachMechanism".into(),
        "-XX:ActiveProcessorCount=2".into(),
        "-XX:+UseSerialGC".into(),
        "-XX:-CreateCoredumpOnCrash".into(),
        r"-XX:ErrorFile=$EW_SCRATCH\jvm-error.log".into(),
        "-Djava.io.tmpdir=$EW_SCRATCH".into(),
        "-Duser.home=$EW_SCRATCH".into(),
        "-Dfile.encoding=UTF-8".into(),
        "-Dworkbench.assignedInput=$EW_INPUT".into(),
    ];
    match job.operation {
        Operation::Index { .. } => arguments.push(r"-Dworkbench.index=$EW_SCRATCH\index".into()),
        Operation::Search { .. } => arguments.push("-Dworkbench.index=$EW_INDEX".into()),
        Operation::Parse => {}
    }
    arguments.extend([
        "-cp".into(),
        r"$EW_RUNTIME\worker.jar;$EW_RUNTIME\lib\*".into(),
        "workbench.FileWorker".into(),
        job.operation_name().into(),
        "$EW_REQUEST".into(),
    ]);
    let output_bytes = if job.role() == Role::Parser {
        PARSE_BYTES
    } else {
        1024 * 1024
    };
    let metadata = serde_json::to_vec(&serde_json::json!({"protocol_version":1,"job_id":job.id,
        "operation":job.operation_name(),"inputs":["input.json"],"output":"result.json",
        "limits":{"seconds":30,"output_bytes":output_bytes,"pages":100,"pixels":1,
            "archive_members":1000,"archive_depth":0,"expanded_bytes":64*1024*1024}}))
    .map_err(|_| Error::Blocked("fixed request encoding failed"))?;
    Ok(Prepared {
        request: Request {
            runtime: root.into(),
            executable: "java/bin/java.exe".into(),
            arguments,
            input: job.input.clone(),
            scratch_parent: scratch_parent.into(),
            wall_time: Duration::from_secs(30),
            memory_bytes: 768 * 1024 * 1024,
        },
        metadata,
        job,
    })
}
/// Engine-only development entrypoint. Paths identify verified runtime and
/// coordinator scratch; no JVM options, input names or class names are accepted.
pub fn execute(
    root: &Path,
    scratch_parent: &Path,
    job: &Job,
    cancelled: impl Fn() -> bool,
) -> Result<JavaOutput> {
    execute_diagnosed(root, scratch_parent, job, cancelled, None)
}
fn execute_diagnosed(
    root: &Path,
    scratch_parent: &Path,
    job: &Job,
    cancelled: impl Fn() -> bool,
    diagnostics: Option<&mut diagnostics::FailureDiagnostics>,
) -> Result<JavaOutput> {
    let mut prepared = prepare(root, scratch_parent, job)?;
    if diagnostics.is_some() {
        prepared
            .request
            .arguments
            .insert(0, "-Dworkbench.probe=true".into());
    }
    crate::validate(&prepared.request)?;
    bounded(
        prepared.metadata.len() <= 1024 * 1024,
        "fixed metadata exceeds bound",
    )?;
    runtime::verify(root, prepared.job.role())?;
    runtime::ordinary_ancestors(scratch_parent)?;
    #[cfg(windows)]
    {
        let output = crate::windows::run_java(&prepared, &cancelled, diagnostics)?;
        bounded(!cancelled(), "Windows Java job cancelled before acceptance")?;
        Ok(output)
    }
    #[cfg(not(windows))]
    {
        let _ = (prepared, cancelled, diagnostics);
        Err(Error::Blocked(
            "Windows Java recipe requires native AppContainer",
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParseReply {
    protocol_version: u32,
    job_id: Uuid,
    content_sha256: String,
    source_bytes: u64,
    parser: String,
    media_type: String,
    status: String,
    text: String,
    metadata: BTreeMap<String, Vec<String>>,
    limitations: Vec<String>,
    error: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexReply {
    indexed: u64,
    workspace_revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hit {
    id: String,
    name: String,
    score: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchReply {
    workspace_revision: String,
    hits: Vec<Hit>,
    total: u64,
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| Error::Blocked("worker result schema rejected"))
}
pub(crate) fn validate_snapshot(snapshot: &IndexSnapshot) -> Result<()> {
    bounded(
        !snapshot.files.is_empty() && snapshot.files.len() <= INDEX_MEMBERS,
        "index member bound exceeded",
    )?;
    let mut seen = std::collections::BTreeSet::new();
    let mut bytes = 0usize;
    for file in &snapshot.files {
        bounded(
            runtime::component(&file.name) && seen.insert(file.name.to_ascii_lowercase()),
            "index filename rejected",
        )?;
        bytes = bytes
            .checked_add(file.bytes.len())
            .ok_or(Error::Blocked("index size overflow"))?;
        bounded(
            file.bytes.len() <= INDEX_FILE_BYTES && bytes <= INDEX_BYTES,
            "index file/aggregate bound exceeded",
        )?;
        bounded(
            format!("{:x}", Sha256::digest(&file.bytes)) == file.sha256,
            "index snapshot digest mismatch",
        )?;
    }
    bounded(
        snapshot
            .files
            .iter()
            .any(|file| file.name.starts_with("segments_")),
        "index commit file missing",
    )
}
pub(crate) fn accept(job: &Job, bytes: Vec<u8>, files: Vec<IndexFile>) -> Result<JavaOutput> {
    bounded(
        bytes.len() as u64
            <= if job.role() == Role::Parser {
                PARSE_BYTES
            } else {
                1024 * 1024
            },
        "worker result exceeds recipe bound",
    )?;
    let index = match &job.operation {
        Operation::Parse => {
            bounded(files.is_empty(), "unexpected parser derivative")?;
            let result: ParseReply = decode(&bytes)?;
            bounded(
                result.protocol_version == 1
                    && result.job_id == job.id
                    && result.content_sha256 == format!("{:x}", Sha256::digest(&job.input))
                    && result.source_bytes == job.input.len() as u64,
                "parser identity/content binding rejected",
            )?;
            bounded(
                matches!(
                    result.status.as_str(),
                    "complete" | "partial" | "unsupported" | "failed"
                ) && result.text.len() <= 512000
                    && !result.text.contains('\0')
                    && result.parser.len() <= 128
                    && result.media_type.len() <= 256
                    && result.metadata.len() <= 32
                    && result.metadata.iter().all(|(k, v)| {
                        k.len() <= 128 && v.len() <= 8 && v.iter().all(|s| s.len() <= 4096)
                    })
                    && result.limitations.len() <= 32
                    && result.limitations.iter().all(|s| s.len() <= 128)
                    && result.error.as_ref().is_none_or(|s| s.len() <= 128),
                "parser transport bounds rejected",
            )?;
            None
        }
        Operation::Index {
            revision,
            documents,
        } => {
            let result: IndexReply = decode(&bytes)?;
            bounded(
                result.workspace_revision == *revision && result.indexed == documents.len() as u64,
                "index acknowledgement mismatch",
            )?;
            let snapshot = IndexSnapshot {
                revision: *revision,
                documents: documents.clone(),
                files,
            };
            validate_snapshot(&snapshot)?;
            Some(snapshot)
        }
        Operation::Search { snapshot } => {
            bounded(files.is_empty(), "unexpected search derivative")?;
            let result: SearchReply = decode(&bytes)?;
            let mut ids = std::collections::BTreeSet::new();
            bounded(
                result.workspace_revision == snapshot.revision.to_string()
                    && result.hits.len() <= 100
                    && result.total >= result.hits.len() as u64
                    && result.total <= snapshot.documents.len() as u64
                    && result.hits.iter().all(|h| {
                        h.score.is_finite()
                            && h.score >= 0.0
                            && snapshot.documents.get(&h.id) == Some(&h.name)
                            && ids.insert(&h.id)
                    }),
                "search revision or result identity rejected",
            )?;
            None
        }
    };
    Ok(JavaOutput {
        job_id: job.id,
        output_sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes,
        index,
    })
}

#[cfg(test)]
mod tests;
