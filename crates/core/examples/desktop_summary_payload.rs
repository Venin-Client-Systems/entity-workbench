//! Fixed synthetic payload diagnostic. No latency or memory benchmark claim.
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Write},
    path::Path,
};
use workbench_core::{
    domain::Command,
    require,
    store::{hash, Workspace},
    Error, Result,
};

const FIXTURE: &str = "c587e3ad59c11445f780306b9aef370e4d97310cf66728b1aabe5f9819c5aef7";
const MAX_BYTES: u64 = 256 * 1024 * 1024;
struct DigestWriter {
    bytes: u64,
    sha: Sha256,
    limit: u64,
}
impl Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self
            .bytes
            .checked_add(bytes.len() as u64)
            .filter(|count| *count <= self.limit)
            .ok_or_else(|| io::Error::other("Payload diagnostic exceeds byte limit"))?;
        self.sha.update(bytes);
        self.bytes = count;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn measure<T: Serialize>(value: &T) -> Result<Value> {
    let mut writer = DigestWriter {
        bytes: 0,
        sha: Sha256::new(),
        limit: MAX_BYTES,
    };
    serde_json::to_writer(&mut writer, value)?;
    Ok(json!({"bytes":writer.bytes, "sha256":format!("{:x}", writer.sha.finalize())}))
}
fn missing() -> Error {
    Error::Validation("Unexpected canonical diagnostic response shape".into())
}
fn length(value: &Value) -> Result<u64> {
    Ok(value.as_array().ok_or_else(missing)?.len() as u64)
}
fn describe(value: &Value) -> Result<Value> {
    let mut fields = serde_json::Map::new();
    for (name, field) in value["workspace"].as_object().ok_or_else(missing)? {
        fields.insert(name.clone(), measure(field)?);
    }
    Ok(
        json!({"response":measure(value)?, "workspace":measure(&value["workspace"])?,
        "analysis":measure(&value["analysis"])?, "workspace_fields":fields}),
    )
}
// The only normalization is the documented projection: remove exactly the two
// workspace arrays and replace exactly the old analysis vectors/counters.
// Every other workspace field, including source text, remains byte-equivalent.
fn project(mut presentation: Value) -> Result<Value> {
    let workspace = presentation["workspace"]
        .as_object_mut()
        .ok_or_else(missing)?;
    let transactions = workspace.remove("transactions").ok_or_else(missing)?;
    let decisions = workspace.remove("decisions").ok_or_else(missing)?;
    workspace.insert("review_decision_count".into(), json!(length(&decisions)?));
    let mut counts = json!({"accepted":0u64,"pending":0u64,"rejected":0u64,"deferred":0u64});
    for row in transactions.as_array().ok_or_else(missing)? {
        let state = row["review"].as_str().ok_or_else(missing)?;
        let slot = counts.get_mut(state).ok_or_else(missing)?;
        *slot = json!(slot.as_u64().ok_or_else(missing)? + 1);
    }
    let analysis = presentation["analysis"]
        .as_object_mut()
        .ok_or_else(missing)?;
    let checks = analysis.remove("balance_checks").ok_or_else(missing)?;
    let duplicate_rows = analysis
        .remove("duplicate_candidates")
        .ok_or_else(missing)?;
    let pending = analysis.remove("pending").ok_or_else(missing)?;
    require(
        pending == counts["pending"],
        "Legacy pending denominator differs",
    )?;
    analysis.insert("transaction_count".into(), json!(length(&transactions)?));
    analysis.insert("review_counts".into(), counts);
    analysis.insert("duplicate_candidate_row_count".into(), duplicate_rows);
    analysis.insert("balance_check_count".into(), json!(length(&checks)?));
    let mut discrepancies = 0u64;
    for check in checks.as_array().ok_or_else(missing)? {
        if !check["reconciled"].as_bool().ok_or_else(missing)? {
            discrepancies += 1;
        }
    }
    analysis.insert("balance_discrepancy_count".into(), json!(discrepancies));
    for total in analysis
        .get_mut("totals")
        .and_then(Value::as_array_mut)
        .ok_or_else(missing)?
    {
        let total = total.as_object_mut().ok_or_else(missing)?;
        let included = total.remove("transaction_ids").ok_or_else(missing)?;
        let excluded = total.remove("excluded_transfer_ids").ok_or_else(missing)?;
        total.insert("included_count".into(), json!(length(&included)?));
        total.insert("excluded_transfer_count".into(), json!(length(&excluded)?));
    }
    presentation
        .as_object_mut()
        .ok_or_else(missing)?
        .insert("schema_version".into(), json!(1));
    Ok(presentation)
}
fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args().collect();
    require(
        args.len() == 3 && ["prepare", "compare"].contains(&args[2].as_str()),
        "Use isolated synthetic workspace and prepare/compare",
    )?;
    let path = Path::new(&args[1]);
    let original = path.join("originals").join(FIXTURE);
    let metadata = std::fs::symlink_metadata(&original)?;
    require(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() == 5_010_041,
        "Expected the fixed synthetic 100k original",
    )?;
    require(
        hash(&std::fs::read(&original)?) == FIXTURE,
        "Synthetic original digest differs",
    )?;
    let mut workspace = Workspace::open(path)?;
    if args[2] == "prepare" {
        return Ok(
            json!({"event":"prepared", "revision":workspace.revision()?, "fixture_sha256":FIXTURE}),
        );
    }
    let presentation = workspace.dispatch_presentation(Command::View {})?;
    require(
        length(&presentation["workspace"]["transactions"])? == 100_000,
        "Expected exactly 100k canonical rows",
    )?;
    require(
        length(&presentation["workspace"]["evidence"])? == 1
            && presentation["workspace"]["evidence"][0]["id"] == FIXTURE
            && length(&presentation["workspace"]["reports"])? == 0,
        "Expected the frozen single-source/no-report baseline",
    )?;
    let presentation_measure = describe(&presentation)?;
    let expected = project(presentation)?;
    let summary = workspace.dispatch_summary(Command::View {})?;
    require(
        summary == expected,
        "Summary differs from the exact documented legacy projection",
    )?;
    // Strict decoding catches an unexpected DTO extension or accidental empty array.
    let _: workbench_core::desktop_summary::DesktopSummaryResponse =
        serde_json::from_value(expected)?;
    let result = json!({"event":"payload_comparison", "workspace_revision":summary["workspace"]["revision"],
        "projection_equal":true, "presentation":presentation_measure,
        "summary":describe(&summary)?, "ledger_summary":summary["analysis"],
        "review_decision_count":summary["workspace"]["review_decision_count"],
        "fixture_sha256":FIXTURE, "timing_claim":false, "memory_claim":false});
    drop(workspace);
    Ok(result)
}
fn main() {
    match run() {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streaming_count_and_hash_match_actual_escaped_json_and_enforce_limit() {
        let value = json!({"text":"Synthetic café\n\"escaped\"","empty":[]});
        let bytes = serde_json::to_vec(&value).unwrap();
        let measured = measure(&value).unwrap();
        assert_eq!(measured["bytes"], bytes.len());
        assert_eq!(measured["sha256"], hash(&bytes));
        let mut writer = DigestWriter {
            bytes: 0,
            sha: Sha256::new(),
            limit: bytes.len() as u64 - 1,
        };
        assert!(serde_json::to_writer(&mut writer, &value).is_err());
        assert!(writer.bytes <= writer.limit);
    }
}
