use std::net::IpAddr;
use tempfile::TempDir;
use workbench_core::{
    analytics::{self, Proximity},
    domain::*,
    policy,
    store::{hash, Workspace},
};
fn workspace() -> (TempDir, Workspace) {
    let temp = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let w = Workspace::open(temp.path().join("case")).unwrap();
    (temp, w)
}
fn demo() -> (TempDir, Workspace) {
    let (temp, mut w) = workspace();
    w.seed_demo().unwrap();
    (temp, w)
}
#[test]
fn demo_cannot_add_fictional_records_to_an_existing_entity_free_workspace() {
    for existing in ["text", "statement", "report"] {
        let (temp, mut w) = workspace();
        match existing {
            "text" => {
                w.import("source.txt", b"Existing synthetic evidence")
                    .unwrap();
            }
            "statement" => {
                w.import(
                    "statement.csv",
                    include_bytes!("../../../fixtures/statement.csv"),
                )
                .unwrap();
            }
            "report" => {
                w.save_report().unwrap();
            }
            _ => unreachable!(),
        }
        let before = w.view().unwrap();
        assert!(before.entities.is_empty());
        let originals = || {
            let mut values: Vec<_> = std::fs::read_dir(temp.path().join("case/originals"))
                .unwrap()
                .map(|entry| {
                    let path = entry.unwrap().path();
                    (
                        path.file_name().unwrap().to_owned(),
                        std::fs::read(path).unwrap(),
                    )
                })
                .collect();
            values.sort();
            values
        };
        let prior_originals = originals();
        assert!(matches!(
            w.seed_demo(),
            Err(workbench_core::Error::Conflict(_))
        ));
        assert_eq!(
            serde_json::to_value(w.view().unwrap()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        assert_eq!(originals(), prior_originals);
    }
}
#[test]
fn decimal_and_calendar_validation_are_strict() {
    assert_eq!(
        (analytics::amount("0.10").unwrap() + analytics::amount("0.20").unwrap()).to_string(),
        "0.30"
    );
    for invalid in ["1e3", "NaN", "Infinity", "1,000", " 12.00", "1.000000001"] {
        assert!(analytics::amount(invalid).is_err(), "{invalid}");
    }
    assert!(analytics::date("2025-02-29").is_err());
    assert!(analytics::date("2024-02-29").is_ok());
}
#[test]
fn malformed_import_is_atomic() {
    let (_t, mut w) = workspace();
    let invalid=b"account,date,description,amount,currency\na,2025-03-01,first,1.00,AUD\na,2025-03-02,broken,nan,AUD\n";
    assert!(w.import("bad.csv", invalid).is_err());
    assert!(w.view().unwrap().transactions.is_empty());
    assert_eq!(w.revision().unwrap(), 0);
}
#[test]
fn import_is_idempotent_and_does_not_delete_repeated_purchases() {
    let (_t, mut w) = workspace();
    let source = include_bytes!("../../../fixtures/statement.csv");
    w.import("first.csv", source).unwrap();
    let revision = w.revision().unwrap();
    w.import("copy.csv", source).unwrap();
    let view = w.view().unwrap();
    assert_eq!(view.transactions.len(), 10);
    assert_eq!(view.evidence.len(), 1);
    assert_eq!(w.revision().unwrap(), revision);
    let repeated: Vec<_> = view
        .transactions
        .iter()
        .filter(|t| t.description == "North Quay Market")
        .collect();
    assert_eq!(repeated.len(), 2);
    assert_eq!(repeated[1].duplicate_candidates.len(), 1);
    let altered =
        String::from_utf8_lossy(source).replace("Opening credit", "Opening credit renamed");
    w.import("overlap.csv", altered.as_bytes()).unwrap();
    assert_eq!(w.view().unwrap().transactions.len(), 20);
    assert!(
        w.view().unwrap().transactions[11]
            .duplicate_candidates
            .len()
            >= 2
    );
}
#[test]
fn review_correction_propagates_without_changing_original_or_snapshot() {
    let (temp, mut w) = demo();
    let before = w.view().unwrap();
    let tx = before
        .transactions
        .iter()
        .find(|t| t.description.contains("OCR"))
        .unwrap();
    let original = std::fs::read(
        temp.path()
            .join("case/originals")
            .join(tx.anchor.evidence_id()),
    )
    .unwrap();
    w.save_report().unwrap();
    let old_report = w.view().unwrap().reports[0].clone();
    w.correct_transaction(
        &tx.id,
        "-18.00",
        "Balance and source reviewed",
        w.revision().unwrap(),
    )
    .unwrap();
    let view = w.view().unwrap();
    assert_eq!(
        view.transactions
            .iter()
            .find(|t| t.id == tx.id)
            .unwrap()
            .amount,
        "-18.00"
    );
    assert!(view.findings.iter().all(|f| f.needs_review));
    assert_eq!(view.reports[0].sha256, old_report.sha256);
    assert_eq!(view.reports[0].html, old_report.html);
    assert_eq!(hash(&original), tx.anchor.evidence_id());
    assert!(analytics::analyse(&view.transactions)
        .unwrap()
        .balance_checks
        .iter()
        .all(|c| c.reconciled));
    assert_eq!(
        std::fs::read(
            temp.path()
                .join("case/originals")
                .join(tx.anchor.evidence_id())
        )
        .unwrap(),
        original
    );
}
#[test]
fn stale_and_reasonless_decisions_are_rejected() {
    let (_t, mut w) = demo();
    let view = w.view().unwrap();
    let key = &view.transactions[0].id;
    assert!(w
        .review_transaction(key, ReviewState::Accepted, "", view.revision)
        .is_err());
    w.review_transaction(key, ReviewState::Accepted, "Source checked", view.revision)
        .unwrap();
    assert!(w
        .review_transaction(key, ReviewState::Rejected, "Stale review", view.revision)
        .is_err());
    assert_eq!(
        w.view().unwrap().transactions[0].review,
        ReviewState::Accepted
    );
}
#[test]
fn transfer_exclusion_requires_matching_reviewed_opposite_amounts() {
    let (_t, mut w) = demo();
    let view = w.view().unwrap();
    let a = &view.transactions[3].id;
    let b = &view.transactions[4].id;
    assert!(w
        .match_transfer(a, b, "Counterpart confirmed", w.revision().unwrap())
        .is_err());
    w.review_transaction(
        a,
        ReviewState::Accepted,
        "Source checked",
        w.revision().unwrap(),
    )
    .unwrap();
    w.review_transaction(
        b,
        ReviewState::Accepted,
        "Source checked",
        w.revision().unwrap(),
    )
    .unwrap();
    w.match_transfer(a, b, "Counterpart confirmed", w.revision().unwrap())
        .unwrap();
    let totals = analytics::analyse(&w.view().unwrap().transactions)
        .unwrap()
        .totals;
    assert_eq!(totals[0].net, "0");
    assert_eq!(totals[0].excluded_transfer_ids.len(), 2);
    assert!(totals[0].transaction_ids.is_empty());
    assert!(w
        .match_transfer(a, b, "Duplicate matching attempt", w.revision().unwrap())
        .is_err());
    w.correct_transaction(a, "-499.00", "Amount corrected", w.revision().unwrap())
        .unwrap();
    assert!(w
        .view()
        .unwrap()
        .transactions
        .iter()
        .all(|t| t.transfer_peer.is_none()));
}
#[test]
fn currencies_are_never_summed_together() {
    let (_t, mut w) = demo();
    let keys: Vec<_> = w
        .view()
        .unwrap()
        .transactions
        .iter()
        .map(|t| t.id.clone())
        .collect();
    for key in keys {
        w.review_transaction(
            &key,
            ReviewState::Accepted,
            "Synthetic source checked",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    let analysis = analytics::analyse(&w.view().unwrap().transactions).unwrap();
    assert_eq!(analysis.totals.len(), 2);
    assert_eq!(analysis.totals[1].currency, "USD");
    assert_eq!(analysis.totals[1].net, "-12.30");
}
#[test]
fn namesakes_retain_leading_zeros_and_merge_can_be_reversed() {
    let (_t, mut w) = demo();
    let before = w.view().unwrap();
    assert_eq!(before.entities[0].identifiers[0].value, "000042");
    w.merge(
        "person-a",
        "person-b",
        "Synthetic mistaken decision",
        w.revision().unwrap(),
    )
    .unwrap();
    let view = w.view().unwrap();
    assert_eq!(view.observations.len(), before.observations.len());
    assert!(w
        .merge(
            "person-b",
            "person-a",
            "Would create a cycle",
            w.revision().unwrap()
        )
        .is_err());
    w.reverse_merge(
        &view.merges[0].id,
        "Conflicting birth year found",
        w.revision().unwrap(),
    )
    .unwrap();
    let after = w.view().unwrap();
    assert!(after.entities.iter().all(|e| e.merged_into.is_none()));
    assert!(after.merges[0].reversed);
}
#[test]
fn backup_restores_database_and_all_referenced_evidence() {
    let (temp, mut w) = demo();
    let backup = w.backup().unwrap();
    let before = w.view().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    let after = restored.view().unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.evidence.len(), before.evidence.len());
    assert_eq!(after.transactions.len(), 10);
    for e in after.evidence {
        assert_eq!(
            hash(&std::fs::read(temp.path().join("restored/originals").join(e.sha256)).unwrap()),
            e.id
        );
    }
    assert!(Workspace::restore(&backup, &temp.path().join("restored")).is_err());
}
#[test]
fn corrupt_original_blocks_backup_and_report() {
    let (temp, mut w) = demo();
    let file = temp
        .path()
        .join("case/originals")
        .join(&w.view().unwrap().evidence[0].sha256);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(file, b"tampered").unwrap();
    assert!(w.backup().is_err());
    assert!(w.save_report().is_err());
}
#[test]
fn newer_schema_is_refused() {
    let (temp, w) = workspace();
    drop(w);
    let path = temp.path().join("case/workspace.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 999).unwrap();
    drop(conn);
    assert!(Workspace::open(temp.path().join("case")).is_err());
}
#[cfg(unix)]
#[test]
fn symlink_workspace_is_refused() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    std::fs::create_dir(temp.path().join("other")).unwrap();
    symlink(temp.path().join("other"), temp.path().join("case")).unwrap();
    assert!(Workspace::open(temp.path().join("case")).is_err());
}
#[test]
fn report_escapes_active_content_and_has_no_scripts() {
    let (_t, mut w) = workspace();
    w.import(
        "attack.txt",
        b"<script>alert('x')</script><img src=https://example.com>",
    )
    .unwrap();
    w.save_report().unwrap();
    let view = w.view().unwrap();
    let html = &view.reports[0].html;
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("<img"));
    assert_eq!(hash(html.as_bytes()), view.reports[0].sha256);
}
#[test]
fn ambiguous_online_and_out_of_date_locations_are_uncertain() {
    let (_t, w) = demo();
    let view = w.view().unwrap();
    let address = &view.addresses[0];
    let mut locations = view.locations;
    assert!(matches!(
        analytics::proximity("2025-03-03", address, &locations, 500.0).unwrap(),
        Proximity::Uncertain { .. }
    ));
    locations[0].review = ReviewState::Accepted;
    locations[0].valid_from = Some("2020-01-01".into());
    assert!(matches!(
        analytics::proximity("2025-03-03", address, &locations, 500.0).unwrap(),
        Proximity::Resolved { .. }
    ));
    assert!(matches!(
        analytics::proximity("2025-03-08", address, &locations, 500.0).unwrap(),
        Proximity::Uncertain { .. }
    ));
    locations[0].channel = Channel::Online;
    assert!(matches!(
        analytics::proximity("2025-03-03", address, &locations, 500.0).unwrap(),
        Proximity::Uncertain { .. }
    ));
    assert!(analytics::proximity("2025-03-03", address, &locations, f64::NAN).is_err());
}
#[test]
fn network_destination_checks_reject_private_and_special_use_addresses() {
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "192.168.1.1",
        "100.64.0.1",
        "::1",
        "fe80::1",
        "fc00::1",
        "::ffff:127.0.0.1",
        "2001:db8::1",
        "2002:7f00:1::1",
    ] {
        assert!(!policy::public_ip(ip.parse::<IpAddr>().unwrap()), "{ip}");
    }
    assert!(policy::validate_destination(
        "https://example.com/",
        &["93.184.215.14".parse().unwrap()],
        &["example.com"]
    )
    .is_ok());
    for url in [
        "http://example.com/",
        "https://user:pass@example.com/",
        "https://example.com:444/",
        "https://other.example/",
    ] {
        assert!(policy::validate_destination(
            url,
            &["93.184.215.14".parse().unwrap()],
            &["example.com"]
        )
        .is_err());
    }
    assert!(policy::validate_destination(
        "https://example.com/",
        &[
            "93.184.215.14".parse().unwrap(),
            "127.0.0.1".parse().unwrap()
        ],
        &["example.com"]
    )
    .is_err());
}
#[test]
fn worker_messages_and_archive_paths_fail_closed() {
    for p in [
        "../escape",
        "/absolute",
        "C:\\temp",
        "ok/../../bad",
        "x\0y",
        ".hidden",
    ] {
        assert!(policy::safe_relative(p).is_err());
    }
    assert!(policy::safe_relative("inputs/document.pdf").is_ok());
    assert!(
        policy::validate_worker_request(b"{\"protocol_version\":1,\"shell\":\"unsafe\"}").is_err()
    );
    assert!(policy::validate_worker_request(&vec![b' '; policy::MAX_MESSAGE_BYTES + 1]).is_err());
    assert!(
        serde_json::from_str::<Command>(r#"{"action":"view","sql":"DELETE FROM records"}"#)
            .is_err()
    );
}
#[test]
fn discovery_stops_at_first_budget_and_never_silently_expands() {
    let mut b = policy::DiscoveryBudget {
        hops: 2,
        requests: 2,
        seconds: 10,
        used: 0,
    };
    b.reserve(0, 0, 5).unwrap();
    b.reserve(2, 9, 1).unwrap();
    assert!(b.reserve(1, 9, 1).is_err());
    let mut b = policy::DiscoveryBudget {
        hops: 2,
        requests: 50,
        seconds: 600,
        used: 0,
    };
    assert!(b.reserve(3, 0, 10).is_err());
    assert!(b.reserve(0, 600, 10).is_err());
    assert!(b.reserve(0, 0, 0).is_err());
    assert_eq!(b.used, 0);
}
#[test]
fn html_is_extracted_without_scripts_or_offsite_link_authority() {
    let (text,links)=workbench_core::collection::extract_html("<p>Public fact</p><script>secret()</script><svg><text>active</text></svg><a href='/next'>Lead</a><a href='javascript:evil()'>bad</a>",&url::Url::parse("https://example.com/start").unwrap());
    assert!(text.contains("Public fact"));
    assert!(!text.contains("secret"));
    assert!(!text.contains("active"));
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].as_str(), "https://example.com/next");
}

#[test]
fn missing_balances_include_all_intervening_rows_without_crossing_accounts() {
    let (_temp, mut w) = workspace();
    w.import("gaps.csv", b"account,date,description,amount,currency,balance\na,2025-03-01,Opening,100.00,AUD,100.00\na,2025-03-02,First purchase,-10.00,AUD,\nb,2025-03-02,Separate account,500.00,AUD,500.00\na,2025-03-03,Second purchase,-20.00,AUD,70.00\na,2025-03-04,Incorrect closing,-5.00,AUD,60.00\n").unwrap();
    let view = w.view().unwrap();
    let checks = analytics::analyse(&view.transactions)
        .unwrap()
        .balance_checks;
    assert_eq!(checks.len(), 2);
    assert!(checks[0].reconciled);
    assert_eq!(
        checks[0].transaction_ids,
        vec![
            view.transactions[1].id.clone(),
            view.transactions[3].id.clone()
        ]
    );
    assert_eq!(checks[1].difference, "-5.00");
    assert!(!checks[1].reconciled);
}
