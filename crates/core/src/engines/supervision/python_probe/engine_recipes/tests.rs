use super::*;

fn result(recipe: Recipe) -> serde_json::Value {
    serde_json::json!({"schema_version":1,"recipe":recipe.identity(),"job_id":"fixed","manifest_sha256":MANIFEST,
        "python_version":"3.13.15","isolated":true,"no_site":true,"no_bytecode":true,"verified_paths":true,
        "checks":recipe.expected().unwrap()})
}
#[test]
fn engine_result_requires_exact_original_assertions_and_recipe_identity() {
    for recipe in [Recipe::Networkx, Recipe::Transactions] {
        let value = result(recipe);
        recipe
            .accept(&serde_json::to_vec(&value).unwrap(), "fixed")
            .unwrap();
        for (pointer, replacement) in [
            ("/recipe", serde_json::json!("python-compatibility-v1")),
            ("/job_id", serde_json::json!("other")),
            ("/checks/versions/spacy", serde_json::json!("wrong")),
            ("/isolated", serde_json::json!(false)),
        ] {
            let mut altered = value.clone();
            *altered.pointer_mut(pointer).unwrap() = replacement;
            assert!(recipe
                .accept(&serde_json::to_vec(&altered).unwrap(), "fixed")
                .is_err());
        }
        let mut altered = value.clone();
        altered["checks"]["unexpected"] = true.into();
        assert!(recipe
            .accept(&serde_json::to_vec(&altered).unwrap(), "fixed")
            .is_err());
        let raw = serde_json::to_string(&value)
            .unwrap()
            .replace("\"spacy\":", "\"spacy\":\"duplicate\",\"spacy\":");
        assert!(recipe.accept(raw.as_bytes(), "fixed").is_err());
        let peer = if recipe == Recipe::Networkx {
            Recipe::Transactions
        } else {
            Recipe::Networkx
        };
        assert!(peer
            .accept(&serde_json::to_vec(&value).unwrap(), "fixed")
            .is_err());
    }
    let mut graph = result(Recipe::Networkx);
    graph["checks"]["graph_unreachable"] = 1.into();
    assert!(Recipe::Networkx
        .accept(&serde_json::to_vec(&graph).unwrap(), "fixed")
        .is_err());
    let mut totals = result(Recipe::Transactions);
    totals["checks"]["transaction_totals"]["totals"][0]["net"] = "1.12345679".into();
    assert!(Recipe::Transactions
        .accept(&serde_json::to_vec(&totals).unwrap(), "fixed")
        .is_err());
}
fn job() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("scratch")).unwrap();
    root
}
fn timing(root: &Path, index: usize, elapsed: u64) {
    let (stage, boundary) = TIMES[index];
    fs::write(root.join(format!("scratch/engine-time-{index}.json")),
        serde_json::to_vec(&serde_json::json!({"stage":stage,"boundary":boundary,"elapsed_ms":elapsed,"process_cpu_ms":elapsed})).unwrap()).unwrap();
}
#[test]
fn engine_diagnostics_keep_valid_prefix_but_reject_malformed_or_gapped_tail() {
    for bad in [br#"{"stage":"operation","boundary":"before","elapsed_ms":0,"process_cpu_ms":0}"#.as_slice(),
        br#"{"stage":"private","boundary":"ready","elapsed_ms":2,"process_cpu_ms":2}"#,
        br#"{"stage":"initialization","stage":"initialization","boundary":"ready","elapsed_ms":2,"process_cpu_ms":2}"#,
        br#"{"stage":"initialization","boundary":"ready","elapsed_ms":2,"process_cpu_ms":true}"#,
        br#"{"stage":"initialization","boundary":"ready","elapsed_ms":120001,"process_cpu_ms":2}"#,
        b"{truncated", &[b' '; 513]] {
        let root = job(); timing(root.path(),0,1);
        fs::write(root.path().join("scratch/engine-time-1.json"), bad).unwrap();
        let got = collect(&root.path().canonicalize().unwrap());
        assert!(!got.valid); assert_eq!(got.checkpoints.len(),1); assert!(!got.complete());
    }
    let root = job();
    timing(root.path(), 0, 1);
    timing(root.path(), 2, 3);
    let got = collect(&root.path().canonicalize().unwrap());
    assert!(!got.valid);
    assert_eq!(got.checkpoints.len(), 1);
}
#[test]
fn engine_diagnostics_enforce_four_records_links_and_monotonic_clocks() {
    let root = job();
    for i in 0..4 {
        timing(root.path(), i, 1);
    }
    assert!(collect(&root.path().canonicalize().unwrap()).complete());
    fs::write(root.path().join("scratch/engine-time-4.json"), b"{}").unwrap();
    assert!(!collect(&root.path().canonicalize().unwrap()).valid);
    fs::remove_file(root.path().join("scratch/engine-time-4.json")).unwrap();
    timing(root.path(), 3, 0);
    assert!(!collect(&root.path().canonicalize().unwrap()).valid);
    fs::remove_file(root.path().join("scratch/engine-time-3.json")).unwrap();
    std::os::unix::fs::symlink(
        "engine-time-0.json",
        root.path().join("scratch/engine-time-3.json"),
    )
    .unwrap();
    assert!(!collect(&root.path().canonicalize().unwrap()).valid);
}
#[test]
fn recipes_keep_role_assets_and_compiled_fixture_identity() {
    let graph = Recipe::Networkx.assets();
    let totals = Recipe::Transactions.assets();
    assert!(!graph
        .iter()
        .any(|(name, _, _)| *name == "code/transaction_totals.py"));
    assert!(totals
        .iter()
        .any(|(name, bytes, _)| *name == "code/transaction_totals.py" && *bytes == ADAPTER));
    for assets in [graph, totals] {
        assert!(!assets
            .iter()
            .any(|(name, _, _)| *name == "code/import_diagnostics.py"));
        assert!(assets
            .iter()
            .any(|(name, bytes, _)| *name == "input/expected.json" && *bytes == EXPECTED));
    }
    assert_eq!(Recipe::Compatibility.phases(), &super::super::PHASES);
}
