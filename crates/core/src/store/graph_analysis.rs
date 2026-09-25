//! Inert internal analytical seam: Rust captures canonical inputs and validates results.
//! No command, worker launch, persistence or accepted-fact mutation is provided here.
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod capture;
mod model;
use model::*;
pub(crate) use model::{CapturedGraph, Hop, ValidatedGraph, ValidatedPath};

const MAX_NODES: usize = 1_000;
const MAX_ASSERTIONS: usize = 5_000;
const MAX_OBSERVATIONS: usize = 10_000;
const MAX_EVIDENCE: usize = 1_000;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_CANONICAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_ORIGINAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_RESULT_BYTES: usize = 128 * 1024;
const RECIPE: &str = "shortest_connection_path_v1";
const POLICY: &str = "accepted_undirected_all_retained_time_v1";
const ENGINE: &str = "networkx";
const ENGINE_VERSION: &str = "3.6.1";
const RUNTIME: &str = "4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822";
const LIMITATION: &str = "Undirected connectivity across accepted assertions from all retained time periods. A path may combine disjoint historical periods and does not establish a contemporaneous, directed or causal relationship.";

impl Workspace {
    /// Only this canonical reader can construct a capture. No paths/SQL/snapshot claims are accepted.
    pub(crate) fn capture_graph_path(
        &self,
        expected_revision: u64,
        source_id: &str,
        target_id: &str,
    ) -> Result<CapturedGraph> {
        require(source_id != target_id, "Graph path endpoints must differ")?;
        capture::identifier(source_id)?;
        capture::identifier(target_id)?;
        let transaction = self.conn.unchecked_transaction()?;
        let revision = capture::revision(&transaction)?;
        same_revision(expected_revision, revision)?;
        let selection = capture::read(self, &transaction, source_id, target_id)?;
        let snapshot_sha256 = selection.digest(revision)?;
        let nonce = Uuid::new_v4().to_string();
        let request = WorkerInput {
            schema_version: 1,
            recipe: RECIPE,
            policy: POLICY,
            nonce: &nonce,
            workspace_revision: revision,
            snapshot_sha256: &snapshot_sha256,
            engine: ENGINE,
            engine_version: ENGINE_VERSION,
            runtime_manifest_sha256: RUNTIME,
            source_id,
            target_id,
            nodes: selection.nodes.iter().map(String::as_str).collect(),
            edges: selection
                .edges
                .keys()
                .map(|(a, b)| [a.as_str(), b.as_str()])
                .collect(),
        };
        let request_bytes = bounded_json(&request, MAX_INPUT_BYTES)?;
        transaction.commit()?;
        Ok(CapturedGraph {
            owner: self.graph_capture_owner,
            revision,
            nonce,
            snapshot_sha256,
            source_id: source_id.into(),
            target_id: target_id.into(),
            request_bytes,
            selection,
        })
    }

    /// Consume the Rust-owned handle even on rejection. This returns no publication authority.
    pub(crate) fn validate_graph_path(
        &self,
        captured: CapturedGraph,
        worker_bytes: &[u8],
    ) -> Result<ValidatedGraph> {
        if captured.owner != self.graph_capture_owner {
            return Err(Error::Conflict(
                "Graph capture belongs to another workspace instance".into(),
            ));
        }
        let transaction = self.conn.unchecked_transaction()?;
        same_revision(captured.revision, capture::revision(&transaction)?)?;
        // Recapture from the pinned canonical read transaction, including original checks.
        // A digest supplied by a worker is never used to choose or authenticate this state.
        let current = capture::read(self, &transaction, &captured.source_id, &captured.target_id)?;
        if current.digest(captured.revision)? != captured.snapshot_sha256 {
            return Err(Error::Conflict(
                "Captured graph records or provenance changed".into(),
            ));
        }
        let result = validate_worker(&captured, worker_bytes)?;
        transaction.commit()?;
        Ok(ValidatedGraph { captured, result })
    }
}

fn same_revision(expected: u64, actual: u64) -> Result<()> {
    if expected != actual {
        return Err(Error::Conflict(
            "Graph snapshot is stale; capture current canonical records".into(),
        ));
    }
    Ok(())
}

fn invalid(message: &str) -> Error {
    Error::InvalidWorkerResult(message.into())
}
fn validate_worker(captured: &CapturedGraph, bytes: &[u8]) -> Result<ValidatedPath> {
    if bytes.len() > MAX_RESULT_BYTES {
        return Err(invalid("Graph result exceeds byte bound"));
    }
    let result: WorkerResult =
        serde_json::from_slice(bytes).map_err(|_| invalid("Malformed graph result"))?;
    if result.schema_version != 1
        || result.recipe != RECIPE
        || result.policy != POLICY
        || result.nonce != captured.nonce
        || result.workspace_revision != captured.revision
        || result.snapshot_sha256 != captured.snapshot_sha256
        || result.engine != ENGINE
        || result.engine_version != ENGINE_VERSION
        || result.runtime_manifest_sha256 != RUNTIME
    {
        return Err(invalid(
            "Graph result identity differs from the owned capture",
        ));
    }
    let distance = shortest_distance(
        &captured.selection,
        &captured.source_id,
        &captured.target_id,
    );
    match result.outcome {
        WorkerOutcome::Unreachable {} => {
            if distance.is_some() {
                return Err(invalid("Graph result incorrectly claims unreachable"));
            }
            Ok(ValidatedPath::Unreachable)
        }
        WorkerOutcome::Path { nodes } => {
            if nodes.len() < 2
                || nodes.len() > MAX_NODES
                || nodes.first() != Some(&captured.source_id)
                || nodes.last() != Some(&captured.target_id)
                || nodes
                    .iter()
                    .any(|id| !captured.selection.nodes.contains(id))
                || nodes.iter().collect::<BTreeSet<_>>().len() != nodes.len()
                || distance != Some(nodes.len() - 1)
            {
                return Err(invalid(
                    "Graph path endpoints, membership or shortest length are invalid",
                ));
            }
            let mut hops = Vec::new();
            for pair in nodes.windows(2) {
                let key = edge_key(&pair[0], &pair[1]);
                let assertions = captured
                    .selection
                    .edges
                    .get(&key)
                    .ok_or_else(|| invalid("Graph path uses an absent edge"))?;
                hops.push(Hop {
                    from: pair[0].clone(),
                    to: pair[1].clone(),
                    assertion_ids: assertions.clone(),
                });
            }
            Ok(ValidatedPath::Path { nodes, hops })
        }
    }
}

fn edge_key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.into(), b.into())
    } else {
        (b.into(), a.into())
    }
}

fn shortest_distance(selection: &Selection, source: &str, target: &str) -> Option<usize> {
    let mut adjacent: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (a, b) in selection.edges.keys() {
        adjacent.entry(a).or_default().push(b);
        adjacent.entry(b).or_default().push(a);
    }
    let mut seen = BTreeSet::from([source]);
    let mut pending = VecDeque::from([(source, 0)]);
    while let Some((node, distance)) = pending.pop_front() {
        if node == target {
            return Some(distance);
        }
        for next in adjacent.get(node).into_iter().flatten() {
            if seen.insert(*next) {
                pending.push_back((next, distance + 1));
            }
        }
    }
    None
}

fn bounded_json(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>> {
    struct Limited {
        bytes: Vec<u8>,
        maximum: usize,
    }
    impl Write for Limited {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if data.len() > self.maximum.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("Graph JSON byte limit"));
            }
            self.bytes.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Limited {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut output, value)
        .map_err(|_| Error::QuotaExhausted("Graph JSON exceeds byte bound".into()))?;
    Ok(output.bytes)
}

#[cfg(test)]
pub(crate) mod probe_fixture;
#[cfg(test)]
mod tests;
