//! Closed test-only recipes. All process, filesystem and cleanup boundaries live in the parent.
use super::*;

const SOURCE: &[u8] = include_bytes!("../../../../../../workers/python/probe/engine_recipes.py");
const PHASES: [&str; 6] = [
    "bootstrap",
    "versions",
    "imports",
    "init-ready",
    "operation",
    "complete",
];
const TIMES: [(&str, &str); 4] = [
    ("initialization", "before"),
    ("initialization", "ready"),
    ("operation", "before"),
    ("operation", "after"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Recipe {
    Compatibility,
    Networkx,
    Transactions,
    CanonicalGraph,
}

pub(super) type AssignedAsset<'a> = (&'static str, &'a [u8], usize);
impl Recipe {
    pub(super) fn identity(self) -> &'static str {
        match self {
            Self::Compatibility => "python-compatibility-v1",
            Self::Networkx => "python-networkx-v1",
            Self::Transactions => "python-transactions-v1",
            Self::CanonicalGraph => "python-canonical-graph-v1",
        }
    }
    pub(super) fn phases(self) -> &'static [&'static str] {
        if self == Self::Compatibility {
            &super::PHASES
        } else if self == Self::CanonicalGraph {
            &canonical_graph::PHASES
        } else {
            &PHASES
        }
    }
    pub(super) fn assets(self) -> Vec<AssignedAsset<'static>> {
        let mut assets = vec![
            ("code/bootstrap.py", BOOTSTRAP, 64 * 1024),
            (
                "code/runtime_support.py",
                include_bytes!("../../../../../../workers/python/runtime_support.py"),
                64 * 1024,
            ),
            ("code/compatibility.py", COMPATIBILITY, 64 * 1024),
            ("input/fixture.json", FIXTURE, 64 * 1024),
        ];
        if self == Self::Compatibility {
            assets.push(("code/import_diagnostics.py", IMPORT_DIAGNOSTICS, 64 * 1024));
            assets.push((
                "input/reader.json",
                br#"{"synthetic":true,"reference":"000123"}"#,
                64 * 1024,
            ));
        } else if self == Self::CanonicalGraph {
            assets.push(("code/engine_recipes.py", SOURCE, 64 * 1024));
            assets.push((
                "code/canonical_graph.py",
                canonical_graph::SOURCE,
                64 * 1024,
            ));
            assets.push((
                "code/graph_path.py",
                canonical_graph::GRAPH_ADAPTER,
                64 * 1024,
            ));
        } else {
            assets.push(("code/engine_recipes.py", SOURCE, 64 * 1024));
            assets.push(("input/expected.json", EXPECTED, 64 * 1024));
        }
        if matches!(self, Self::Compatibility | Self::Transactions) {
            assets.push(("code/transaction_totals.py", ADAPTER, 32 * 1024));
        }
        assets
    }
    pub(super) fn expected(self) -> Result<serde_json::Value> {
        let expected: Checks = serde_json::from_slice(EXPECTED)?;
        Ok(match self {
            Self::CanonicalGraph => {
                return Err(Error::Validation(
                    "Canonical recipe requires owned capture".into(),
                ))
            }
            Self::Compatibility => serde_json::to_value(expected)?,
            Self::Networkx => {
                serde_json::json!({"versions":expected.versions,"imported_modules":["networkx"],
                "graph_path":expected.graph_path,"graph_assertions":expected.graph_assertions,"graph_unreachable":expected.graph_unreachable})
            }
            Self::Transactions => {
                serde_json::json!({"versions":expected.versions,"imported_modules":["duckdb","pyarrow","pyarrow.compute","pyarrow.parquet"],
                "transaction_totals":expected.transaction_totals,"transfer_rejected":expected.transfer_rejected})
            }
        })
    }
    pub(super) fn accept(self, bytes: &[u8], job_id: &str) -> Result<serde_json::Value> {
        require(
            self != Self::CanonicalGraph,
            "Canonical recipe requires owned capture",
        )?;
        if self == Self::Compatibility {
            return Ok(serde_json::to_value(super::accept(bytes, job_id)?)?);
        }
        require(
            bytes.len() as u64 <= RESULT_LIMIT,
            "Engine probe result exceeds limit",
        )?;
        let result: WorkerResult<EngineChecks> = serde_json::from_slice(bytes)?;
        require(
            result.schema_version == 1
                && result.recipe == self.identity()
                && result.job_id == job_id
                && result.manifest_sha256 == MANIFEST
                && result.python_version == "3.13.15"
                && result.isolated
                && result.no_site
                && result.no_bytecode
                && result.verified_paths
                && serde_json::to_value(&result.checks)? == self.expected()?,
            "Engine probe assertions failed",
        )?;
        Ok(serde_json::to_value(result)?)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum EngineChecks {
    Networkx(GraphChecks),
    Transactions(TransactionChecks),
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphChecks {
    #[serde(deserialize_with = "unique_versions")]
    versions: BTreeMap<String, String>,
    imported_modules: Vec<String>,
    graph_path: Vec<String>,
    graph_assertions: Vec<String>,
    graph_unreachable: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransactionChecks {
    #[serde(deserialize_with = "unique_versions")]
    versions: BTreeMap<String, String>,
    imported_modules: Vec<String>,
    transaction_totals: Totals,
    transfer_rejected: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Timing {
    stage: String,
    boundary: String,
    elapsed_ms: u64,
    process_cpu_ms: u64,
}
#[derive(Debug, Default, Serialize)]
pub(super) struct Diagnostics {
    valid: bool,
    checkpoints: Vec<Timing>,
}
impl Diagnostics {
    pub(super) fn complete(&self) -> bool {
        self.valid && self.checkpoints.len() == TIMES.len()
    }
}
fn read_timings(job: &Path, result: &mut Diagnostics) -> Result<()> {
    let mut previous = (0, 0);
    let mut missing = false;
    for (index, (stage, boundary)) in TIMES.iter().enumerate() {
        let path = job.join(format!("scratch/engine-time-{index}.json"));
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                continue;
            }
            other => {
                other?;
            }
        }
        require(!missing, "Engine timing gap")?;
        let record: Timing = serde_json::from_slice(&read_result(&path, 512)?)?;
        require(
            record.stage == *stage
                && record.boundary == *boundary
                && record.elapsed_ms >= previous.0
                && record.process_cpu_ms >= previous.1
                && record.elapsed_ms <= 120_000
                && record.process_cpu_ms <= 120_000,
            "Invalid engine timing",
        )?;
        previous = (record.elapsed_ms, record.process_cpu_ms);
        result.checkpoints.push(record);
    }
    let allowed: BTreeSet<_> = (0..4)
        .map(|index| format!("engine-time-{index}.json"))
        .collect();
    for (index, item) in fs::read_dir(job.join("scratch"))?.enumerate() {
        require(index < 512, "Engine timing directory bound")?;
        let name = item?.file_name();
        if let Some(name) = name
            .to_str()
            .filter(|name| name.starts_with("engine-time-"))
        {
            require(allowed.contains(name), "Unexpected engine timing record")?;
        }
    }
    Ok(())
}
pub(super) fn collect(job: &Path) -> Diagnostics {
    let mut result = Diagnostics::default();
    if reject_linked_ancestors(&job.join("scratch")).is_ok() {
        result.valid = read_timings(job, &mut result).is_ok();
    }
    result
}

#[test]
#[ignore = "requires explicit reviewed macOS-arm64 candidate execution"]
fn native_python_networkx() {
    native_recipe(Recipe::Networkx);
}
#[test]
#[ignore = "requires explicit reviewed macOS-arm64 candidate execution"]
fn native_python_transactions() {
    native_recipe(Recipe::Transactions);
}

#[cfg(test)]
mod tests;
