//! Exercise the actual development entry point; native transport changes separately.
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
fn invoke(root: &Path, mode: Option<&str>, command: Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ew-dev"));
    child.arg(root);
    if let Some(mode) = mode {
        child.arg(mode);
    }
    let mut child = child
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&command).unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn summary_flag_controls_view_and_mutation_but_keeps_legacy_and_direct_responses() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let csv = b"account,date,description,amount,currency\n0001,2025-01-01,Synthetic CLI export,-0.10000001,AUD\n";
    let imported = invoke(
        &root,
        Some("--summary"),
        json!({"action":"import","name":"synthetic.csv","bytes":csv.to_vec()}),
    );
    assert_eq!(imported["schema_version"], 1);
    assert_eq!(imported["workspace"]["revision"], 1);
    assert!(imported["workspace"].get("transactions").is_none());
    assert_eq!(imported["analysis"]["transaction_count"], 1);
    let summary = invoke(&root, Some("--summary"), json!({"action":"view"}));
    assert_eq!(summary, imported);
    let legacy = invoke(&root, None, json!({"action":"view"}));
    let presentation = invoke(&root, Some("--presentation"), json!({"action":"view"}));
    assert_eq!(legacy, presentation); // No reports in this fixture.
    assert_eq!(
        legacy["workspace"]["transactions"][0]["amount"],
        "-0.10000001"
    );
    assert!(legacy["workspace"].get("decisions").is_some());
    assert!(summary["workspace"].get("decisions").is_none());
    let request = json!({"action":"export_transactions","request":{"query":"","filter":{"date_from":null,"date_to":null,"account":null,"currency":null,"review":null},"order":"date_ascending"},"expected_revision":1});
    let direct = invoke(&root, None, request.clone());
    assert_eq!(
        direct,
        invoke(&root, Some("--presentation"), request.clone())
    );
    assert_eq!(direct, invoke(&root, Some("--summary"), request));
    assert!(direct.get("workspace").is_none());
    assert_eq!(direct["row_count"], 1);
}
