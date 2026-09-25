//! Synthetic cross-language fixture; no Python invocation or production hook.
use super::*;

fn fixed_workspace() -> (TempDir, Workspace) {
    let (root, mut workspace, source) = specimen();
    workspace
        .change(
            Some(workspace.revision().unwrap()),
            "synthetic.graph.protocol",
            false,
            |conn| {
                let mut evidence: Evidence = get(conn, "evidence", &source)?;
                evidence.imported_at = "2026-09-26T00:00:00Z".into();
                put(conn, "evidence", &source, &evidence)
            },
        )
        .unwrap();
    (root, workspace)
}

#[test]
fn canonical_graph_protocol_fixture() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../workers/python/fixtures/canonical-graph-cases.v1.json"
    )))
    .unwrap();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 3);
    let (_root, workspace) = fixed_workspace();
    let revision = workspace.revision().unwrap();
    for (index, (name, source, target)) in [
        ("connected", "a", "c"),
        ("reverse", "c", "a"),
        ("unreachable", "a", "f"),
    ]
    .into_iter()
    .enumerate()
    {
        let case = &fixture["cases"][index];
        assert_eq!(case["name"], name);
        let capture = workspace
            .capture_graph_path(revision, source, target)
            .unwrap();
        let mut request: Value = serde_json::from_slice(capture.worker_input()).unwrap();
        let actual_nonce = request["nonce"].clone();
        // Only the fresh per-capture nonce is normalized for fixture equality.
        // Revision, actual record fingerprint, runtime identity and graph remain exact.
        request["nonce"] = case["request"]["nonce"].clone();
        assert_eq!(request, case["request"]);
        let mut result = case["result"].clone();
        assert_eq!(result["nonce"], case["request"]["nonce"]);
        result["nonce"] = actual_nonce;
        let validated = workspace
            .validate_graph_path(capture, &serde_json::to_vec(&result).unwrap())
            .unwrap();
        assert_eq!(validated.workspace_revision(), revision);
        if name == "unreachable" {
            assert_eq!(validated.path(), &ValidatedPath::Unreachable);
        } else {
            let ValidatedPath::Path { nodes, hops } = validated.path() else {
                panic!("expected path")
            };
            assert_eq!(
                serde_json::to_value(nodes).unwrap(),
                case["result"]["outcome"]["nodes"]
            );
            let parallel = hops
                .iter()
                .find(|hop| hop.assertion_ids.contains(&"r1".to_owned()))
                .unwrap();
            assert_eq!(parallel.assertion_ids, ["r1", "r1-parallel"]);
        }
    }
}
