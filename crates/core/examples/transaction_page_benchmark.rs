//! Fixed synthetic paging measurements; development-only, never an application command.
// Shared frozen generator also contains setup-only fields and functions.
#[path = "transaction_page_benchmark/empty_query.rs"]
mod empty_query;
#[allow(dead_code)]
#[path = "performance_baseline/fixture.rs"]
mod fixture;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path, time::Instant};
use workbench_core::{
    domain::{Command, ReviewState, SourceAnchor},
    require,
    store::Workspace,
    transaction_page::{
        TransactionPage, TransactionPageFilter, TransactionPageOrder, TransactionPageRequest,
        TransactionReviewCounts,
    },
    Error, Result,
};

const ROWS: usize = 100_000;
const SAMPLES: usize = 21;
const OPERATIONS: &[&str] = &[
    "all_first",
    "all_next",
    "descending_first",
    "filtered_first",
    "filtered_next",
    "pending_first",
    "empty_first",
    "stale_revision",
    "presentation",
];
#[cfg(test)]
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn emit(value: Value) {
    println!("{value}");
    std::io::stdout().flush().unwrap();
}
fn request(operation: &str) -> Result<TransactionPageRequest> {
    require(OPERATIONS.contains(&operation), "Unknown fixed measurement")?;
    let mut request = TransactionPageRequest {
        page_size: 200,
        ..Default::default()
    };
    if operation.starts_with("filtered") {
        request.filter = TransactionPageFilter {
            account: Some("0006".into()),
            currency: Some("AUD".into()),
            date_from: Some("2021-01-01".into()),
            date_to: Some("2021-12-31".into()),
            review: Some(ReviewState::Accepted),
        };
    } else if operation == "pending_first" {
        request.filter.review = Some(ReviewState::Pending);
    } else if operation == "empty_first" {
        request.filter.account = Some("SYNTHETIC-NONEXISTENT".into());
    } else if operation == "descending_first" {
        request.order = TransactionPageOrder::DateDescending;
    }
    request.validate()?;
    Ok(request)
}
struct Oracle {
    counts: TransactionReviewCounts,
    selected: Vec<usize>,
}
fn oracle(request: &TransactionPageRequest) -> Oracle {
    let mut counts = TransactionReviewCounts::default();
    let mut selected = Vec::new();
    let f = &request.filter;
    for index in 0..ROWS {
        let row = fixture::row(index);
        if f.account.as_ref().is_some_and(|v| *v != row.account)
            || f.currency.as_ref().is_some_and(|v| *v != row.currency)
            || f.date_from.as_ref().is_some_and(|v| row.date < *v)
            || f.date_to.as_ref().is_some_and(|v| row.date > *v)
        {
            continue;
        }
        match row.review {
            ReviewState::Accepted => counts.accepted += 1,
            ReviewState::Pending => counts.pending += 1,
            ReviewState::Rejected => counts.rejected += 1,
            ReviewState::Deferred => counts.deferred += 1,
        }
        if f.review.as_ref().is_none_or(|v| *v == row.review) {
            selected.push((index, row.date));
        }
    }
    selected.sort_by(|a, b| {
        let dates = match request.order {
            TransactionPageOrder::DateAscending => a.1.cmp(&b.1),
            TransactionPageOrder::DateDescending => b.1.cmp(&a.1),
        };
        dates.then(a.0.cmp(&b.0))
    });
    Oracle {
        counts,
        selected: selected.into_iter().map(|r| r.0).collect(),
    }
}
fn check(page: &TransactionPage, expected: &Oracle, offset: usize, revision: u64) -> Result<()> {
    require(
        page.schema_version == 1 && page.workspace_revision == revision,
        "Page version differs",
    )?;
    require(
        page.review_counts == expected.counts,
        "Page review denominators differ",
    )?;
    require(
        page.scope_count
            == expected.counts.accepted
                + expected.counts.pending
                + expected.counts.rejected
                + expected.counts.deferred,
        "Page scope count differs",
    )?;
    require(
        page.selected_count == expected.selected.len() as u64,
        "Page selected count differs",
    )?;
    let end = (offset + 200).min(expected.selected.len());
    let indices = &expected.selected[offset..end];
    require(page.rows.len() == indices.len(), "Page row count differs")?;
    require(
        page.next_cursor.is_some() == (end < expected.selected.len()),
        "Page continuation differs",
    )?;
    for (actual, &index) in page.rows.iter().zip(indices) {
        let row = fixture::row(index);
        let source_row = (index + 2) as u32;
        require(
            actual.id == format!("{}:{source_row}", fixture::frozen_sha256(ROWS))
                && actual.account == row.account
                && actual.date == row.date
                && actual.description == row.description
                && actual.amount == fixture::money(row.cents)
                && actual.currency == row.currency
                && actual.review == row.review
                && actual.version > 0
                && actual.balance.is_none()
                && actual.posting_date.is_none(),
            "Page canonical row differs from frozen fixture",
        )?;
        require(
            matches!(&actual.anchor, SourceAnchor::Cell { evidence_id, sheet, row, column }
            if evidence_id == fixture::frozen_sha256(ROWS) && *row == source_row && sheet == "CSV" && column == "amount"),
            "Page original anchor differs",
        )?;
    }
    Ok(())
}
struct Counter {
    bytes: u64,
    hash: Sha256,
}
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes += bytes.len() as u64;
        if self.bytes > 256 * 1024 * 1024 {
            return Err(std::io::Error::other("Response exceeds diagnostic bound"));
        }
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn serialize(value: &Value) -> Result<(u64, String)> {
    let mut counter = Counter {
        bytes: 0,
        hash: Sha256::new(),
    };
    serde_json::to_writer(&mut counter, value)?;
    Ok((counter.bytes, format!("{:x}", counter.hash.finalize())))
}
fn run(directory: &Path, operation: &str) -> Result<()> {
    if operation == "empty_query_diagnostic" {
        return empty_query::run(directory);
    }
    let mut workspace = Workspace::open(directory)?;
    let revision = workspace.revision()?;
    if operation == "prepare" {
        let request = request("all_first")?;
        let page = workspace.page_transactions(&request, revision)?;
        check(&page, &oracle(&request), 0, revision)?;
        require(
            page.scope_count == ROWS as u64,
            "Frozen corpus size differs",
        )?;
        emit(
            json!({"event":"prepared", "workspace_revision":revision, "rows":ROWS,
            "fixture_sha256":fixture::frozen_sha256(ROWS)}),
        );
        return Ok(());
    }
    let mut request = request(operation)?;
    let expected = oracle(&request);
    let offset = if operation.ends_with("_next") {
        let first = workspace.page_transactions(&request, revision)?;
        check(&first, &expected, 0, revision)?;
        request.cursor = first.next_cursor;
        require(
            request.cursor.is_some(),
            "Continuation needs a preceding page",
        )?;
        200
    } else {
        0
    };
    emit(
        json!({"event":"started", "operation":operation, "workspace_revision":revision,
        "request":request, "offset":offset, "samples":SAMPLES}),
    );
    for sample in 0..SAMPLES {
        let command = if operation == "presentation" {
            Command::View {}
        } else {
            Command::PageTransactions {
                request: request.clone(),
                expected_revision: if operation == "stale_revision" {
                    revision - 1
                } else {
                    revision
                },
            }
        };
        let started = Instant::now();
        let result = workspace.dispatch_presentation(command);
        if operation == "stale_revision" {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            require(
                matches!(result, Err(Error::Conflict(_))),
                "Stale request did not conflict",
            )?;
            require(
                workspace.revision()? == revision,
                "Stale request changed revision",
            )?;
            emit(
                json!({"event":"sample", "operation":operation, "sample":sample,
                "elapsed_ms":elapsed_ms, "response_bytes":null, "oracle_passed":true,
                "expected_error":"conflict"}),
            );
            continue;
        }
        let value = result?;
        let (response_bytes, response_sha256) = serialize(&value)?;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        if operation == "presentation" {
            let view = &value["workspace"];
            require(
                view["revision"] == revision
                    && view["transactions"]
                        .as_array()
                        .is_some_and(|v| v.len() == ROWS)
                    && view["evidence"].as_array().is_some_and(|v| v.len() == 1)
                    && view["evidence"][0]["sha256"] == fixture::frozen_sha256(ROWS),
                "Presentation corpus differs",
            )?;
        } else {
            check(
                &serde_json::from_value::<TransactionPage>(value)?,
                &expected,
                offset,
                revision,
            )?;
        }
        require(
            workspace.revision()? == revision,
            "Measurement changed revision",
        )?;
        emit(
            json!({"event":"sample", "operation":operation, "sample":sample,
            "elapsed_ms":elapsed_ms, "response_bytes":response_bytes,
            "response_sha256":response_sha256, "oracle_passed":true}),
        );
    }
    emit(json!({"event":"complete", "operation":operation, "samples":SAMPLES}));
    Ok(())
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let result = (|| {
        require(
            args.len() == 3,
            "Usage: transaction_page_benchmark WORKSPACE FIXED_OPERATION",
        )?;
        run(Path::new(&args[1]), &args[2])
    })();
    if let Err(error) = result {
        emit(json!({"event":"failure", "detail":error.to_string()}));
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_scope_oracle_keeps_denominators_and_continuation() {
        let all = oracle(&request("all_first").unwrap());
        assert_eq!(all.counts.accepted, 85_000);
        assert_eq!(all.counts.pending, 5_000);
        assert_eq!(all.selected.len(), ROWS);
        let filtered = oracle(&request("filtered_first").unwrap());
        assert!(filtered.selected.len() > 400);
        assert_eq!(filtered.selected.len() as u64, filtered.counts.accepted);
        assert_eq!(
            oracle(&request("pending_first").unwrap()).selected.len(),
            5_000
        );
        assert!(oracle(&request("empty_first").unwrap()).selected.is_empty());
        assert!(request("arbitrary").is_err());
    }
    #[test]
    fn payload_counter_matches_real_serializer() {
        let value = json!({"escaped":"\"\\\n", "unicode":"é", "array":[1,null]});
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            serialize(&value).unwrap(),
            (bytes.len() as u64, digest(&bytes))
        );
    }
}
