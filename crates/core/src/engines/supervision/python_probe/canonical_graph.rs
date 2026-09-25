//! One fixed native test recipe. Lifecycle and confinement stay in the parent supervisor.
use super::*;
use crate::store::graph_analysis::probe_fixture::CanonicalProbe;

pub(super) const SOURCE: &[u8] =
    include_bytes!("../../../../../../workers/python/probe/canonical_graph.py");
pub(super) const GRAPH_ADAPTER: &[u8] =
    include_bytes!("../../../../../../workers/python/graph_path.py");
pub(super) const PHASES: [&str; 7] = [
    "bootstrap",
    "versions",
    "metadata",
    "imports",
    "init-ready",
    "operation",
    "complete",
];
const GRAPH_LIMIT: u64 = 128 * 1024;

pub(super) struct Context {
    owned: CanonicalProbe,
    campaign: String,
    capture: serde_json::Value,
}
impl Context {
    pub(super) fn new(artifacts: &Path, campaign: &str) -> Result<Self> {
        let owned = CanonicalProbe::new(artifacts)?;
        let capture = owned.metadata()?;
        super::super::super::write_new(&artifacts.join("captured-request.json"), owned.request())?;
        Ok(Self {
            owned,
            campaign: campaign.into(),
            capture,
        })
    }
    pub(super) fn request(&self) -> &[u8] {
        self.owned.request()
    }
    pub(super) fn capture(&self) -> serde_json::Value {
        self.capture.clone()
    }
    pub(super) fn assignment(&self, value: &mut serde_json::Value) {
        value["campaign_id"] = self.campaign.clone().into();
        value["capture_nonce"] = self.capture["nonce"].clone();
        value["request_identity"] = self.capture["request_identity"].clone();
    }
    pub(super) fn accept(
        &mut self,
        raw: &[u8],
        graph_raw: &[u8],
        job_id: &str,
    ) -> Result<serde_json::Value> {
        require(
            raw.len() <= 64 * 1024 && graph_raw.len() as u64 <= GRAPH_LIMIT,
            "Canonical probe result exceeds bound",
        )?;
        let result: WorkerResult<Checks> = serde_json::from_slice(raw)?;
        let expected: super::Checks = serde_json::from_slice(EXPECTED)?;
        require(
            result.schema_version == 1
                && result.recipe == Recipe::CanonicalGraph.identity()
                && result.job_id == job_id
                && result.manifest_sha256 == MANIFEST
                && result.python_version == "3.13.15"
                && result.isolated
                && result.no_site
                && result.no_bytecode
                && result.verified_paths
                && result.checks.versions == expected.versions
                && result.checks.imported_modules == ["networkx"]
                && result.checks.backend_metadata_checked
                && result.checks.campaign_id == self.campaign
                && serde_json::to_value(&result.checks.capture_nonce)? == self.capture["nonce"]
                && serde_json::to_value(&result.checks.request_identity)?
                    == self.capture["request_identity"]
                && result.checks.result_identity.bytes == graph_raw.len() as u64
                && result.checks.result_identity.sha256 == digest(graph_raw),
            "Canonical probe identity differs",
        )?;
        // Validate the original bytes, never a JSON Value reserialization that can discard duplicates.
        let canonical = self.owned.accept(graph_raw)?;
        Ok(serde_json::json!({"worker":result,"canonical":canonical}))
    }
    pub(super) fn read_accept(
        &mut self,
        job: &Path,
        artifacts: &Path,
        job_id: &str,
        observation: &mut serde_json::Value,
    ) -> Result<serde_json::Value> {
        let raw = read_result(&job.join("scratch/result.json"), 64 * 1024)?;
        observation["wrapper_output_identity"] =
            serde_json::json!({"bytes":raw.len(),"sha256":digest(&raw)});
        super::super::super::write_new(&artifacts.join("captured-wrapper.json"), &raw)?;
        let graph_raw = read_result(&job.join("scratch/graph-result.json"), GRAPH_LIMIT)?;
        observation["graph_output_identity"] =
            serde_json::json!({"bytes":graph_raw.len(),"sha256":digest(&graph_raw)});
        super::super::super::write_new(&artifacts.join("captured-result.json"), &graph_raw)?;
        self.accept(&raw, &graph_raw, job_id)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileIdentity {
    bytes: u64,
    sha256: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Checks {
    #[serde(deserialize_with = "unique_versions")]
    versions: BTreeMap<String, String>,
    imported_modules: Vec<String>,
    backend_metadata_checked: bool,
    campaign_id: String,
    capture_nonce: String,
    request_identity: FileIdentity,
    result_identity: FileIdentity,
}

#[test]
#[ignore = "requires explicit reviewed macOS-arm64 canonical graph execution"]
fn native_python_canonical_graph() {
    native_recipe(Recipe::CanonicalGraph);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Context, serde_json::Value, Vec<u8>) {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let context = Context::new(root.path(), "fixed-campaign").unwrap();
        let mut graph: serde_json::Value = serde_json::from_slice(context.request()).unwrap();
        for key in ["source_id", "target_id", "nodes", "edges"] {
            graph.as_object_mut().unwrap().remove(key);
        }
        graph["outcome"] = serde_json::json!({"state":"path","nodes":["a","b","c"]});
        let bytes = serde_json::to_vec(&graph).unwrap();
        let expected: super::super::Checks = serde_json::from_slice(EXPECTED).unwrap();
        let result = serde_json::json!({"schema_version":1,"recipe":Recipe::CanonicalGraph.identity(),
            "job_id":"fixed-job","manifest_sha256":MANIFEST,"python_version":"3.13.15",
            "isolated":true,"no_site":true,"no_bytecode":true,"verified_paths":true,
            "checks":{"versions":expected.versions,"imported_modules":["networkx"],"backend_metadata_checked":true,
            "campaign_id":"fixed-campaign","capture_nonce":context.capture["nonce"],
            "request_identity":context.capture["request_identity"],"result_identity":{"bytes":bytes.len(),"sha256":digest(&bytes)}}});
        (root, context, result, bytes)
    }
    #[test]
    fn canonical_graph_requires_owned_capture_and_exact_independent_correlations() {
        let (_root, mut context, result, graph) = fixture();
        let good = serde_json::to_vec(&result).unwrap();
        assert!(Recipe::CanonicalGraph.accept(&good, "fixed-job").is_err());
        assert!(Recipe::CanonicalGraph.expected().is_err());
        for (field, value) in [("campaign_id", "other"), ("capture_nonce", "other")] {
            let mut bad = result.clone();
            bad["checks"][field] = value.into();
            assert!(context
                .accept(&serde_json::to_vec(&bad).unwrap(), &graph, "fixed-job")
                .is_err());
        }
        assert!(context.accept(&good, &graph, "other-job").is_err());
        let mut changed = graph.clone();
        changed.push(b' ');
        assert!(context.accept(&good, &changed, "fixed-job").is_err());
        let value = context.accept(&good, &graph, "fixed-job").unwrap();
        assert_eq!(value["canonical"]["canonical_unchanged"], true);
        assert!(context.accept(&good, &graph, "fixed-job").is_err());
    }
    #[test]
    fn canonical_graph_raw_duplicates_unknowns_and_oversize_are_not_reserialized_away() {
        let (_root, mut context, result, graph) = fixture();
        let text = serde_json::to_string(&result).unwrap();
        let duplicate = text.replacen(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
            1,
        );
        assert!(context
            .accept(duplicate.as_bytes(), &graph, "fixed-job")
            .is_err());
        let mut extra = result.clone();
        extra["checks"]["private"] = true.into();
        assert!(context
            .accept(&serde_json::to_vec(&extra).unwrap(), &graph, "fixed-job")
            .is_err());
        assert!(context
            .accept(&vec![b' '; 64 * 1024 + 1], &graph, "fixed-job")
            .is_err());
        assert!(context
            .accept(
                text.as_bytes(),
                &vec![b' '; GRAPH_LIMIT as usize + 1],
                "fixed-job"
            )
            .is_err());
        let duplicate_graph = String::from_utf8(graph)
            .unwrap()
            .replacen(
                "\"schema_version\":1",
                "\"schema_version\":1,\"schema_version\":1",
                1,
            )
            .into_bytes();
        let mut wrapper = result;
        wrapper["checks"]["result_identity"] =
            serde_json::json!({"bytes":duplicate_graph.len(),"sha256":digest(&duplicate_graph)});
        assert!(context
            .accept(
                &serde_json::to_vec(&wrapper).unwrap(),
                &duplicate_graph,
                "fixed-job"
            )
            .is_err());
    }
}
