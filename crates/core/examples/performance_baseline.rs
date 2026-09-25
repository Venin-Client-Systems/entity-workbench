//! Development measurement executable; never shipped or exposed through application commands.
#[path = "performance_baseline/fixture.rs"]
mod fixture;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Write, path::Path, time::Instant};
use workbench_core::{
    domain::{Command, ReviewState},
    require,
    store::Workspace,
    transaction_analysis::{TransactionAnalysisRequest, TransferTreatment},
    transaction_comparison::{DatePeriod, TransactionComparisonRequest},
    Result,
};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn emit(value: Value) {
    println!("{value}");
    std::io::stdout().flush().unwrap();
}
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn normalized_money(value: &str) -> &str {
    if value.contains('.') {
        value.trim_end_matches('0').trim_end_matches('.')
    } else {
        value
    }
}

fn setup(root: &Path, rows: usize) -> Result<()> {
    require(
        matches!(rows, 1000 | 100_000),
        "Use the fixed 1,000-row smoke or 100,000-row baseline",
    )?;
    require(
        !root.join("case").exists(),
        "Setup needs a fresh output directory",
    )?;
    let csv = fixture::csv(rows);
    let sha = digest(&csv);
    require(
        sha == fixture::frozen_sha256(rows),
        "Frozen fixture bytes changed",
    )?;
    fs::write(root.join("fixture.csv"), &csv)?;
    let mut workspace = Workspace::open(root.join("case"))?;
    emit(json!({"event":"started","phase":"canonical_import"}));
    let start = Instant::now();
    require(
        workspace.import("synthetic-performance-v1.csv", &csv)? == sha,
        "Import digest mismatch",
    )?;
    let import_ms = ms(start);
    emit(json!({"event":"phase","phase":"canonical_import","elapsed_ms":import_ms}));
    let start = Instant::now();
    let mut revision = workspace.revision()?;
    let mut counts = BTreeMap::new();
    emit(json!({"event":"started","phase":"canonical_review"}));
    for i in 0..rows {
        let review = fixture::row(i).review;
        let name = serde_json::to_value(&review)?.as_str().unwrap().to_string();
        *counts.entry(name).or_insert(0usize) += 1;
        if review != ReviewState::Pending {
            workspace.review_transaction(
                &format!("{sha}:{}", i + 2),
                review,
                "Fixed synthetic performance fixture decision; no analyst assessment",
                revision,
            )?;
            revision += 1;
        }
        if (i + 1) % 10_000 == 0 {
            emit(
                json!({"event":"progress","phase":"canonical_review","rows":i+1,"elapsed_ms":ms(start)}),
            );
        }
    }
    if rows == 100_000 {
        for month in 0..100 {
            workspace.match_transfer(
                &format!("{sha}:{}", 99_800 + month + 2),
                &format!("{sha}:{}", 99_900 + month + 2),
                "Fixed synthetic reciprocal transfer decision",
                revision,
            )?;
            revision += 1;
        }
    }
    let review_ms = ms(start);
    // Independent readback before a backup becomes the measurement baseline.
    let view = workspace.view()?;
    require(
        view.transactions.len() == rows && view.revision == revision,
        "Setup row/revision mismatch",
    )?;
    let mut actual = BTreeMap::new();
    for t in &view.transactions {
        *actual
            .entry(
                serde_json::to_value(&t.review)?
                    .as_str()
                    .unwrap()
                    .to_string(),
            )
            .or_insert(0usize) += 1;
    }
    require(actual == counts, "Canonical review-state counts differ")?;
    drop(view);
    let start = Instant::now();
    let backup = workspace.backup()?;
    let backup_ms = ms(start);
    let manifest = json!({"fixture_version":fixture::VERSION,"rows":rows,"fixture_bytes":csv.len(),
        "fixture_sha256":sha,"review_counts":counts,"workspace_revision":revision,
        "reviewed_transfer_pairs":if rows==100_000 {100} else {0},
        "import_ms":import_ms,"canonical_review_ms":review_ms,"backup_ms":backup_ms,
        "backup":backup.strip_prefix(root).unwrap().to_string_lossy()});
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    emit(json!({"event":"setup_complete","fixture":manifest}));
    Ok(())
}

fn command(operation: &str, revision: u64) -> Result<Command> {
    let command = match operation {
        "view" => Command::View {},
        "patterns_all" => Command::AnalyzeTransactions {
            expected_revision: revision,
            request: TransactionAnalysisRequest::default(),
        },
        "patterns_filtered" | "patterns_empty" => Command::AnalyzeTransactions {
            expected_revision: revision,
            request: TransactionAnalysisRequest {
                date_from: Some("2021-01-01".into()),
                date_to: Some("2021-12-31".into()),
                account: Some(
                    if operation == "patterns_empty" {
                        "NO-SUCH-SYNTHETIC-ACCOUNT"
                    } else {
                        "0006"
                    }
                    .into(),
                ),
                currency: Some("AUD".into()),
                transfers: TransferTreatment::ExcludeReviewedPairs,
                ..Default::default()
            },
        },
        "comparison" => Command::CompareTransactionPeriods {
            expected_revision: revision,
            request: TransactionComparisonRequest {
                baseline: DatePeriod {
                    from: "2020-01-01".into(),
                    through: "2020-12-31".into(),
                },
                comparison: DatePeriod {
                    from: "2021-01-01".into(),
                    through: "2021-12-31".into(),
                },
                account: None,
                currency: None,
                transfers: TransferTreatment::ExcludeReviewedPairs,
            },
        },
        _ => {
            return Err(workbench_core::Error::Validation(
                "Unknown fixed benchmark operation".into(),
            ))
        }
    };
    Ok(command)
}

#[derive(Default)]
struct Expected {
    credits: i64,
    debits: i64,
    accepted: Vec<String>,
    pending: Vec<String>,
    rejected: Vec<String>,
    deferred: Vec<String>,
    excluded: Vec<String>,
}
impl Expected {
    fn add(&mut self, r: &fixture::Row, id: String, exclude: bool) {
        match r.review {
            ReviewState::Pending => self.pending.push(id),
            ReviewState::Rejected => self.rejected.push(id),
            ReviewState::Deferred => self.deferred.push(id),
            ReviewState::Accepted if r.transfer && exclude => self.excluded.push(id),
            ReviewState::Accepted => {
                self.accepted.push(id);
                if r.cents < 0 {
                    self.debits -= r.cents;
                } else {
                    self.credits += r.cents;
                }
            }
        }
    }
    fn check(&self, value: &Value) -> Result<()> {
        let total = &value["total"];
        for (field, expected) in [
            ("credits", self.credits),
            ("debits", self.debits),
            ("net", self.credits - self.debits),
        ] {
            require(
                total[field].as_str().is_some_and(|actual| {
                    normalized_money(actual) == normalized_money(&fixture::money(expected))
                }),
                "Integer-cent total oracle differs",
            )?;
        }
        for (actual, expected) in [
            (&total["transaction_ids"], &self.accepted),
            (&value["pending_ids"], &self.pending),
            (&value["rejected_ids"], &self.rejected),
            (&value["deferred_ids"], &self.deferred),
            (&value["excluded_transfer_ids"], &self.excluded),
        ] {
            let mut actual: Vec<String> = serde_json::from_value(actual.clone())?;
            actual.sort();
            let mut expected = expected.clone();
            expected.sort();
            require(
                actual == expected,
                "Complete source/review denominator oracle differs",
            )?;
        }
        Ok(())
    }
}

fn validate(operation: &str, value: &Value, rows: usize, sha: &str, revision: u64) -> Result<()> {
    if operation == "view" {
        return require(
            value["workspace"]["revision"] == revision
                && value["workspace"]["transactions"]
                    .as_array()
                    .is_some_and(|a| a.len() == rows),
            "Canonical view row count/revision differs",
        );
    }
    require(
        value["workspace_revision"] == revision && value["workspace_transaction_count"] == rows,
        "Calculation revision/denominator differs",
    )?;
    let mut expected: BTreeMap<(String, String, String), Expected> = BTreeMap::new();
    let mut scope = 0;
    for i in 0..rows {
        let row = fixture::row(i);
        let period = if operation == "comparison" {
            if row.date.starts_with("2020-") {
                "baseline"
            } else if row.date.starts_with("2021-") {
                "comparison"
            } else {
                continue;
            }
        } else {
            if operation == "patterns_empty"
                || (operation == "patterns_filtered"
                    && (!row.date.starts_with("2021-")
                        || row.account != "0006"
                        || row.currency != "AUD"))
            {
                continue;
            }
            ""
        };
        scope += 1;
        let account = if operation == "comparison" {
            row.account.clone()
        } else {
            String::new()
        };
        expected
            .entry((account, row.currency.into(), period.into()))
            .or_default()
            .add(
                &row,
                format!("{sha}:{}", i + 2),
                operation != "patterns_all",
            );
    }
    if operation == "comparison" {
        let groups = value["groups"]
            .as_array()
            .ok_or_else(|| workbench_core::Error::Validation("Missing comparison groups".into()))?;
        require(
            groups.len() * 2 == expected.len(),
            "Comparison groups differ",
        )?;
        require(
            value["baseline_transaction_count"].as_u64().unwrap_or(0)
                + value["comparison_transaction_count"].as_u64().unwrap_or(0)
                == scope,
            "Comparison scope differs",
        )?;
        for group in groups {
            for period in ["baseline", "comparison"] {
                let key = (
                    group["account"].as_str().unwrap_or("").into(),
                    group["currency"].as_str().unwrap_or("").into(),
                    period.into(),
                );
                expected
                    .remove(&key)
                    .ok_or_else(|| {
                        workbench_core::Error::Validation("Unexpected comparison group".into())
                    })?
                    .check(&group[period])?;
            }
        }
    } else {
        require(
            value["scope_transaction_count"] == scope
                && value["rows"]
                    .as_array()
                    .is_some_and(|a| a.len() as u64 == scope),
            "Pattern scope differs",
        )?;
        let groups = value["currencies"]
            .as_array()
            .ok_or_else(|| workbench_core::Error::Validation("Missing currency groups".into()))?;
        require(groups.len() == expected.len(), "Currency groups differ")?;
        for group in groups {
            let key = (
                String::new(),
                group["currency"].as_str().unwrap_or("").into(),
                String::new(),
            );
            expected
                .remove(&key)
                .ok_or_else(|| {
                    workbench_core::Error::Validation("Unexpected currency group".into())
                })?
                .check(group)?;
        }
    }
    require(expected.is_empty(), "Missing expected groups")
}

fn measure(root: &Path, operation: &str, repetitions: usize) -> Result<()> {
    require(
        (1..=6).contains(&repetitions),
        "Use one to six total samples",
    )?;
    let fixture: Value = serde_json::from_slice(&fs::read(root.join("fixture.json"))?)?;
    let rows = fixture["rows"].as_u64().unwrap() as usize;
    let revision = fixture["workspace_revision"].as_u64().unwrap();
    let sha = fixture["fixture_sha256"].as_str().unwrap();
    require(
        digest(&fixture::csv(rows)) == sha && sha == fixture::frozen_sha256(rows),
        "Fixture identity differs",
    )?;
    let backup = root.join(fixture["backup"].as_str().unwrap());
    let destination = root.join(format!("measure-{operation}"));
    emit(json!({"event":"started","phase":"restore","operation":operation}));
    let start = Instant::now();
    let mut workspace = Workspace::restore(&backup, &destination)?;
    let mut restore_ms = ms(start);
    for sample in 0..repetitions {
        if operation == "html_export" && sample > 0 {
            let start = Instant::now();
            workspace =
                Workspace::restore(&backup, &root.join(format!("measure-{operation}-{sample}")))?;
            restore_ms = ms(start);
        }
        require(
            workspace.revision()? == revision,
            "Read-only baseline revision changed",
        )?;
        emit(json!({"event":"started","phase":"operation","operation":operation,"sample":sample}));
        let start = Instant::now();
        let (bytes, output) = if operation == "html_export" {
            let id = workspace.save_report()?;
            let elapsed_ms = ms(start);
            // Verification/readback deliberately excluded from publication latency.
            let view = workspace.view()?;
            let report = view.reports.iter().find(|r| r.id == id).unwrap();
            require(
                view.reports.len() == 1
                    && report.workspace_revision == revision
                    && digest(report.html.as_bytes()) == report.sha256,
                "Report snapshot binding differs",
            )?;
            let path = if sample == 0 {
                destination.clone()
            } else {
                root.join(format!("measure-{operation}-{sample}"))
            };
            let bytes = fs::read(path.join("exports").join(format!("{id}.html")))?;
            require(
                bytes == report.html.as_bytes(),
                "Canonical/export bytes differ",
            )?;
            (
                bytes,
                json!({"elapsed_ms":elapsed_ms,"snapshot_revision":report.workspace_revision,"review_denominators":fixture["review_counts"]}),
            )
        } else {
            let value = workspace.dispatch(command(operation, revision)?)?;
            let bytes = serde_json::to_vec(&value)?;
            let elapsed_ms = ms(start);
            validate(operation, &value, rows, sha, revision)?;
            require(
                workspace.revision()? == revision,
                "Read calculation mutated workspace",
            )?;
            let summaries = if operation == "view" {
                fixture["review_counts"].clone()
            } else if operation == "comparison" {
                json!(value["groups"].as_array().unwrap().iter().map(|g|json!({
                    "account":g["account"],"currency":g["currency"],
                    "baseline":denominators(&g["baseline"]),"comparison":denominators(&g["comparison"])
                })).collect::<Vec<_>>())
            } else {
                json!(value["currencies"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|g| json!({
                        "currency":g["currency"],"counts":denominators(g)
                    }))
                    .collect::<Vec<_>>())
            };
            (
                bytes,
                json!({"elapsed_ms":elapsed_ms,"review_denominators":summaries,
                "merchant_group_count":value["merchant_groups"].as_array().map(Vec::len),
                "recurring_candidate_count":value["recurring_candidates"].as_array().map(Vec::len)}),
            )
        };
        emit(
            json!({"event":"sample","operation":operation,"sample":sample,
            "cache_label":if sample==0 {"first_call_fresh_process_os_cache_uncontrolled"} else {"repeated_call_same_process_os_cache_uncontrolled"},
            "elapsed_ms":output["elapsed_ms"],"restore_ms":if sample==0||operation=="html_export"{restore_ms}else{0.0},
            "review_denominators":output["review_denominators"],
            "merchant_group_count":output["merchant_group_count"],
            "recurring_candidate_count":output["recurring_candidate_count"],
            "rows":rows,"workspace_revision":revision,"output_bytes":bytes.len(),"output_sha256":digest(&bytes),
            "oracle_passed":true}),
        );
    }
    emit(json!({"event":"complete","operation":operation,"samples":repetitions}));
    Ok(())
}

fn denominators(value: &Value) -> Value {
    let count = |v: &Value| v.as_array().map(Vec::len).unwrap_or(0);
    json!({"accepted_included":count(&value["total"]["transaction_ids"]),
        "pending":count(&value["pending_ids"]),"rejected":count(&value["rejected_ids"]),
        "deferred":count(&value["deferred_ids"]),"reviewed_transfer_excluded":count(&value["excluded_transfer_ids"])})
}

/// A labelled payload diagnostic, separate from timed samples. Avoid another giant JSON buffer.
fn serialized_bytes(value: &Value) -> Result<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len();
            if self.0 > 256 * 1024 * 1024 {
                return Err(std::io::Error::other("Diagnostic JSON exceeds 256 MiB"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.0)
}

fn field_sizes(value: &Value) -> Result<Value> {
    let mut fields = BTreeMap::new();
    let object = value
        .as_object()
        .ok_or_else(|| workbench_core::Error::Validation("Expected response object".into()))?;
    let mut value_bytes = 0;
    for (key, value) in object {
        let bytes = serialized_bytes(value)?;
        value_bytes += bytes;
        fields.insert(
            key,
            json!({"serialized_value_bytes":bytes,"array_items":value.as_array().map(Vec::len)}),
        );
    }
    let total = serialized_bytes(value)?;
    Ok(
        json!({"fields":fields,"serialized_object_bytes":total,"keys_and_punctuation_bytes":total-value_bytes}),
    )
}

fn diagnose(root: &Path, case: &str) -> Result<()> {
    let (directory, report_count) = match case {
        "baseline" => (root.join("case"), 0),
        "one_report" => (root.join("measure-html_export"), 1),
        _ => {
            return Err(workbench_core::Error::Validation(
                "Only fixed retained diagnostic cases are supported".into(),
            ))
        }
    };
    require(
        directory.join("workspace.db").is_file(),
        "Retained diagnostic workspace is missing",
    )?;
    let mut workspace = Workspace::open(directory)?;
    let revision = workspace.revision()?;
    let value = workspace.dispatch(Command::View {})?;
    let view = &value["workspace"];
    require(
        view["transactions"]
            .as_array()
            .is_some_and(|a| a.len() == 100_000)
            && view["reports"]
                .as_array()
                .is_some_and(|a| a.len() == report_count),
        "Diagnostic requires retained 100k baseline or one-report workspace",
    )?;
    let evidence = view["evidence"].as_array().unwrap();
    require(
        evidence.len() == 1 && evidence[0]["sha256"] == fixture::frozen_sha256(100_000),
        "Diagnostic fixture differs",
    )?;
    let mut text = Vec::new();
    for e in evidence {
        text.push(
            json!({"evidence_id":e["id"],"text_raw_utf8_bytes":e["text"].as_str().map(str::len),
            "text_serialized_string_bytes":serialized_bytes(&e["text"])?}),
        );
    }
    let mut reports = Vec::new();
    for report in view["reports"].as_array().unwrap() {
        let html = report["html"].as_str().unwrap();
        require(
            digest(html.as_bytes()) == report["sha256"].as_str().unwrap(),
            "Canonical report digest differs",
        )?;
        reports.push(json!({"id":report["id"],"workspace_revision":report["workspace_revision"],"sha256":report["sha256"],
            "html_raw_utf8_bytes":html.len(),"html_serialized_string_bytes":serialized_bytes(&report["html"])?}));
    }
    require(
        workspace.revision()? == revision,
        "Diagnostic mutated revision",
    )?;
    emit(
        json!({"event":"payload_diagnostic","case":case,"workspace_revision":revision,
        "top_level":field_sizes(&value)?,"workspace":field_sizes(view)?,"evidence_text":text,"reports_html":reports,
        "timing_claim":false,"report_count":report_count}),
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = (|| {
        require(
            args.len() == 5,
            "Usage: performance_baseline setup|measure OUTPUT ROWS|OPERATION 0|SAMPLES",
        )?;
        let root = Path::new(&args[2]);
        if args[1] == "setup" {
            setup(
                root,
                args[3]
                    .parse()
                    .map_err(|_| workbench_core::Error::Validation("Invalid rows".into()))?,
            )
        } else if args[1] == "diagnose" {
            diagnose(root, &args[3])
        } else if args[1] == "measure" {
            measure(
                root,
                &args[3],
                args[4]
                    .parse()
                    .map_err(|_| workbench_core::Error::Validation("Invalid repetitions".into()))?,
            )
        } else {
            Err(workbench_core::Error::Validation("Unknown mode".into()))
        }
    })();
    if let Err(error) = result {
        emit(json!({"event":"failure","detail":error.to_string()}));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_commands_validate_and_reject_unbounded_or_unknown_scopes() {
        for op in [
            "view",
            "patterns_all",
            "patterns_filtered",
            "patterns_empty",
            "comparison",
        ] {
            command(op, 1).unwrap();
        }
        assert!(command("arbitrary", 1).is_err());
    }
    #[test]
    fn money_oracle_rejects_lost_cent_and_missing_source() {
        let e = Expected {
            credits: 101,
            accepted: vec!["source:2".into()],
            ..Default::default()
        };
        let mut value = json!({"total":{"credits":"1.01","debits":"0.00","net":"1.01","transaction_ids":["source:2"]},"pending_ids":[],"rejected_ids":[],"deferred_ids":[],"excluded_transfer_ids":[]});
        e.check(&value).unwrap();
        value["total"]["credits"] = json!("1.00");
        assert!(e.check(&value).is_err());
        value["total"]["credits"] = json!("1.01");
        value["total"]["transaction_ids"] = json!([]);
        assert!(e.check(&value).is_err());
    }
    #[test]
    fn payload_counter_matches_json_escaping_and_complete_object_overhead() {
        let value = json!({"text":"<tag>\n\"escaped\"","rows":[1,2,3],"none":null});
        let sizes = field_sizes(&value).unwrap();
        assert_eq!(
            serialized_bytes(&value).unwrap(),
            serde_json::to_vec(&value).unwrap().len()
        );
        let fields = sizes["fields"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v["serialized_value_bytes"].as_u64().unwrap())
            .sum::<u64>();
        assert_eq!(
            fields + sizes["keys_and_punctuation_bytes"].as_u64().unwrap(),
            serialized_bytes(&value).unwrap() as u64
        );
    }
}
