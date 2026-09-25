use super::*;
use serde::Deserialize;

/// No Clone, Serialize or Deserialize: canonical capture authority cannot arrive in worker JSON.
pub(crate) struct CapturedGraph {
    pub(super) owner: Uuid,
    pub(super) revision: u64,
    pub(super) nonce: String,
    pub(super) snapshot_sha256: String,
    pub(super) source_id: String,
    pub(super) target_id: String,
    pub(super) request_bytes: Vec<u8>,
    pub(super) selection: Selection,
}
impl CapturedGraph {
    pub(crate) fn worker_input(&self) -> &[u8] {
        &self.request_bytes
    }
}

/// Read-only interpretation at the captured revision. Not a durable publication/job capability.
pub(crate) struct ValidatedGraph {
    pub(super) captured: CapturedGraph,
    pub(super) result: ValidatedPath,
}
impl ValidatedGraph {
    pub(crate) fn workspace_revision(&self) -> u64 {
        self.captured.revision
    }
    pub(crate) fn snapshot_sha256(&self) -> &str {
        &self.captured.snapshot_sha256
    }
    pub(crate) fn limitation(&self) -> &'static str {
        LIMITATION
    }
    pub(crate) fn path(&self) -> &ValidatedPath {
        &self.result
    }
    pub(crate) fn provenance(&self) -> &Selection {
        &self.captured.selection
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ValidatedPath {
    Unreachable,
    Path { nodes: Vec<String>, hops: Vec<Hop> },
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Hop {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) assertion_ids: Vec<String>,
}

pub(crate) struct Selection {
    pub(super) nodes: BTreeSet<String>,
    pub(super) edges: BTreeMap<(String, String), Vec<String>>,
    pub(crate) fingerprints: BTreeMap<(String, String), String>,
    pub(crate) assertions: BTreeMap<String, Assertion>,
    pub(crate) observations: BTreeMap<String, Observation>,
    pub(crate) evidence: BTreeMap<String, EvidenceProvenance>,
    pub(crate) assertion_reviews: ReviewCounts,
}
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ReviewCounts {
    pub(crate) accepted: usize,
    pub(crate) pending: usize,
    pub(crate) rejected: usize,
    pub(crate) deferred: usize,
}
impl ReviewCounts {
    pub(super) fn record(&mut self, state: &ReviewState) {
        match state {
            ReviewState::Accepted => self.accepted += 1,
            ReviewState::Pending => self.pending += 1,
            ReviewState::Rejected => self.rejected += 1,
            ReviewState::Deferred => self.deferred += 1,
        }
    }
}

/// Evidence text never enters the capture handle or worker; original identity and origin do.
pub(crate) struct EvidenceProvenance {
    pub(crate) sha256: String,
    pub(crate) bytes: u64,
    pub(crate) origin_group: String,
}
impl Selection {
    pub(super) fn digest(&self, revision: u64) -> Result<String> {
        #[derive(Serialize)]
        struct Identity<'a> {
            revision: u64,
            recipe: &'static str,
            policy: &'static str,
            nodes: Vec<&'a str>,
            edges: Vec<(&'a str, &'a str, &'a [String])>,
            records: Vec<(&'a str, &'a str, &'a str)>,
        }
        let identity = Identity {
            revision,
            recipe: RECIPE,
            policy: POLICY,
            nodes: self.nodes.iter().map(String::as_str).collect(),
            edges: self
                .edges
                .iter()
                .map(|((a, b), ids)| (a.as_str(), b.as_str(), ids.as_slice()))
                .collect(),
            records: self
                .fingerprints
                .iter()
                .map(|((kind, id), sha)| (kind.as_str(), id.as_str(), sha.as_str()))
                .collect(),
        };
        Ok(hash(&bounded_json(&identity, MAX_CANONICAL_BYTES)?))
    }
}

#[derive(Serialize)]
pub(super) struct WorkerInput<'a> {
    pub schema_version: u32,
    pub recipe: &'static str,
    pub policy: &'static str,
    pub nonce: &'a str,
    pub workspace_revision: u64,
    pub snapshot_sha256: &'a str,
    pub engine: &'static str,
    pub engine_version: &'static str,
    pub runtime_manifest_sha256: &'static str,
    pub source_id: &'a str,
    pub target_id: &'a str,
    pub nodes: Vec<&'a str>,
    pub edges: Vec<[&'a str; 2]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkerResult {
    pub schema_version: u32,
    pub recipe: String,
    pub policy: String,
    pub nonce: String,
    pub workspace_revision: u64,
    pub snapshot_sha256: String,
    pub engine: String,
    pub engine_version: String,
    pub runtime_manifest_sha256: String,
    pub outcome: WorkerOutcome,
}
#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum WorkerOutcome {
    Path { nodes: Vec<String> },
    Unreachable {},
}
