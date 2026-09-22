use tempfile::TempDir;
use workbench_core::{
    domain::*,
    statements::*,
    store::{hash, Workspace},
};
fn workspace() -> (TempDir, Workspace) {
    let t = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let w = Workspace::open(t.path().join("case")).unwrap();
    (t, w)
}
fn mapping() -> StatementMapping {
    StatementMapping {
        delimiter: Delimiter::Semicolon,
        date_format: DateFormat::DayFirst,
        number_format: NumberFormat::CommaDecimal,
        row_order: RowOrder::NewestFirst,
        date: "Booked".into(),
        posting_date: None,
        description: "Narrative".into(),
        amount: AmountMapping::DebitCredit {
            debit: "Paid out".into(),
            credit: "Paid in".into(),
        },
        balance: Some("Running balance".into()),
        account: ValueMapping::Column {
            column: "Account ref".into(),
        },
        currency: ValueMapping::Column {
            column: "Unit".into(),
        },
    }
}
const SOURCE: &[u8] = include_bytes!("../../../fixtures/statement-mapped.csv");
fn commit(
    w: &mut Workspace,
    name: &str,
    bytes: &[u8],
    mapping: StatementMapping,
    profile: Option<&str>,
) {
    let preview = w.preview_statement(name, bytes, &mapping).unwrap();
    w.import_statement(
        name,
        bytes,
        mapping,
        &preview.preview_token,
        profile,
        preview.workspace_revision,
    )
    .unwrap();
}
#[test]
fn mapped_import_preserves_decimals_reference_text_and_logical_source_rows() {
    let (temp, mut w) = workspace();
    let preview = w
        .preview_statement("mapped.csv", SOURCE, &mapping())
        .unwrap();
    assert_eq!(
        (
            preview.total_rows,
            preview.valid_rows,
            preview.invalid_rows,
            preview.balance_mismatches
        ),
        (3, 3, 0, 0)
    );
    assert_eq!(
        preview.rows[0].transaction.as_ref().unwrap().amount,
        "12.30"
    );
    assert!(w.view().unwrap().evidence.is_empty());
    assert_eq!(w.revision().unwrap(), 0);
    commit(
        &mut w,
        "mapped.csv",
        SOURCE,
        mapping(),
        Some("Synthetic semicolon statement"),
    );
    let view = w.view().unwrap();
    assert_eq!(view.transactions.len(), 3);
    assert_eq!(view.transactions[0].date, "2025-03-03");
    assert_eq!(view.transactions[0].amount, "1000.00");
    assert!(view
        .transactions
        .iter()
        .all(|t| t.account == "000017" && t.review == ReviewState::Pending));
    let debit = &view.transactions[1];
    assert_eq!(debit.amount, "-12.30");
    assert_eq!(debit.description, "Synthetic books\nreceipt 04");
    assert_eq!(w.inspect_source(&debit.anchor).unwrap().quote, "12,30");
    assert!(w
        .inspect_source(&debit.anchor)
        .unwrap()
        .location
        .contains("row 3"));
    assert_eq!(
        std::fs::read(temp.path().join("case/originals").join(hash(SOURCE))).unwrap(),
        SOURCE
    );
    assert_eq!(
        view.statement_imports[0].profile_id.as_ref(),
        Some(&view.statement_profiles[0].id)
    );
    let backup = w.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(
        restored.inspect_source(&debit.anchor).unwrap().quote,
        "12,30"
    );
    assert_eq!(
        restored.view().unwrap().statement_profiles[0].mapping,
        mapping()
    );
}
#[test]
fn invalid_rows_block_the_entire_import_and_are_reported_beyond_the_sample() {
    let (temp, mut w) = workspace();
    let mut source =
        "Booked;Narrative;Paid out;Paid in;Running balance;Account ref;Unit\n".to_string();
    for _ in 0..52 {
        source.push_str("03/03/2025;Synthetic purchase;1,00;;;000017;AUD\n");
    }
    source.push_str("03/03/2025;Ambiguous movement;12,30;12,30;;000017;AUD\n");
    let preview = w
        .preview_statement("invalid.csv", source.as_bytes(), &mapping())
        .unwrap();
    assert_eq!(preview.valid_rows, 52);
    assert_eq!(preview.invalid_rows, 1);
    assert!(preview.rows_truncated);
    assert_eq!(preview.issues[0].source_row, 54);
    assert!(preview.issues[0].message.contains("Both debit and credit"));
    assert!(w
        .import_statement(
            "invalid.csv",
            source.as_bytes(),
            mapping(),
            &preview.preview_token,
            Some("Bad"),
            0
        )
        .is_err());
    assert_eq!(w.revision().unwrap(), 0);
    assert!(w.view().unwrap().statement_profiles.is_empty());
    assert_eq!(
        std::fs::read_dir(temp.path().join("case/originals"))
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn signed_polarity_grouping_dates_and_constants_are_explicit() {
    let (_temp, mut w) = workspace();
    let bytes=b"Day,Details,Value\n03/04/2025,Synthetic debit,\"1,234.56\"\n04/04/2025,Synthetic refund,(12.30)\n";
    let mut m = StatementMapping {
        delimiter: Delimiter::Comma,
        date_format: DateFormat::DayFirst,
        number_format: NumberFormat::DotDecimal,
        row_order: RowOrder::OldestFirst,
        date: "Day".into(),
        posting_date: None,
        description: "Details".into(),
        amount: AmountMapping::Signed {
            column: "Value".into(),
            positive_is_debit: true,
        },
        balance: None,
        account: ValueMapping::Constant {
            value: "000099".into(),
        },
        currency: ValueMapping::Constant {
            value: "AUD".into(),
        },
    };
    let p = w.preview_statement("signed.csv", bytes, &m).unwrap();
    assert_eq!(p.rows[0].transaction.as_ref().unwrap().amount, "-1234.56");
    assert_eq!(p.rows[1].transaction.as_ref().unwrap().amount, "12.30");
    assert_eq!(p.rows[0].transaction.as_ref().unwrap().date, "2025-04-03");
    m.date_format = DateFormat::MonthFirst;
    let p = w.preview_statement("signed.csv", bytes, &m).unwrap();
    assert_eq!(p.rows[0].transaction.as_ref().unwrap().date, "2025-03-04");
    commit(&mut w, "signed.csv", bytes, m.clone(), None);
    for value in [
        "1,23.45",
        "1e3",
        "NaN",
        "--1.00",
        "( -1.00 )",
        "1.000000001",
    ] {
        let s = format!("Day,Details,Value\n03/04/2025,Synthetic,\"{value}\"\n");
        assert_eq!(
            w.preview_statement("bad.csv", s.as_bytes(), &m)
                .unwrap()
                .invalid_rows,
            1,
            "{value}"
        );
    }
}
#[test]
fn preview_binding_rejects_changed_bytes_mapping_filename_and_workspace() {
    let (_temp, mut w) = workspace();
    let p = w
        .preview_statement("mapped.csv", SOURCE, &mapping())
        .unwrap();
    let mut changed = mapping();
    changed.row_order = RowOrder::OldestFirst;
    assert!(w
        .import_statement("mapped.csv", SOURCE, changed, &p.preview_token, None, 0)
        .is_err());
    assert!(w
        .import_statement("other.csv", SOURCE, mapping(), &p.preview_token, None, 0)
        .is_err());
    let altered = String::from_utf8(SOURCE.to_vec())
        .unwrap()
        .replace("refund", "returned purchase");
    assert!(w
        .import_statement(
            "mapped.csv",
            altered.as_bytes(),
            mapping(),
            &p.preview_token,
            None,
            0
        )
        .is_err());
    w.import("note.txt", b"Synthetic source note").unwrap();
    assert!(w
        .import_statement("mapped.csv", SOURCE, mapping(), &p.preview_token, None, 0)
        .is_err());
    assert!(w.view().unwrap().transactions.is_empty());
}
#[test]
fn same_original_is_not_imported_twice_but_overlapping_sources_remain_reviewable() {
    let (_temp, mut w) = workspace();
    commit(&mut w, "mapped.csv", SOURCE, mapping(), Some("Bank layout"));
    let p = w
        .preview_statement("renamed.csv", SOURCE, &mapping())
        .unwrap();
    assert!(p.already_imported);
    assert!(w
        .import_statement(
            "renamed.csv",
            SOURCE,
            mapping(),
            &p.preview_token,
            None,
            p.workspace_revision
        )
        .is_err());
    let mut other = SOURCE.to_vec();
    other.extend_from_slice(b"06/03/2025;Synthetic extra purchase;1,00;;999,00;000017;AUD\n");
    let profile = w.view().unwrap().statement_profiles[0].clone();
    commit(&mut w, "overlap.csv", &other, profile.mapping, None);
    let v = w.view().unwrap();
    assert_eq!(v.transactions.len(), 7);
    assert_eq!(v.statement_profiles.len(), 1);
    assert!(v
        .statement_imports
        .iter()
        .all(|i| i.profile_id.as_ref() == Some(&profile.id)));
    assert!(v
        .transactions
        .iter()
        .any(|t| !t.duplicate_candidates.is_empty()));
}
#[test]
fn structural_and_mapping_errors_fail_closed() {
    let (_temp, w) = workspace();
    assert!(sample(b"Date,Date\n1,2\n", Delimiter::Comma).is_err());
    assert!(sample(b"Date,,Amount\n1,2,3\n", Delimiter::Comma).is_err());
    assert!(sample(&[255], Delimiter::Comma).is_err());
    let mut m = mapping();
    m.amount = AmountMapping::DebitCredit {
        debit: "Paid out".into(),
        credit: "Paid out".into(),
    };
    assert!(w.preview_statement("bad.csv", SOURCE, &m).is_err());
    m = mapping();
    m.date = "Missing".into();
    assert!(w.preview_statement("bad.csv", SOURCE, &m).is_err());
    assert!(w
        .preview_statement("../source.csv", SOURCE, &mapping())
        .is_err());
    assert!(w
        .preview_statement(
            "empty.csv",
            b"Booked;Narrative;Paid out;Paid in;Running balance;Account ref;Unit\n",
            &mapping()
        )
        .is_err());
}
#[test]
fn schema_one_upgrade_creates_a_recoverable_backup_with_originals() {
    let (temp, mut w) = workspace();
    let original = b"Synthetic retained source";
    let id = w.import("source.txt", original).unwrap();
    let before = w.revision().unwrap();
    drop(w);
    let db = temp.path().join("case/workspace.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    drop(conn);
    let upgraded = Workspace::open(temp.path().join("case")).unwrap();
    assert_eq!(upgraded.view().unwrap().schema_version, 2);
    assert_eq!(upgraded.revision().unwrap(), before + 1);
    let backup = std::fs::read_dir(temp.path().join("case/backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(backup.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(
        std::fs::read(backup.join("originals").join(&id)).unwrap(),
        original
    );
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(restored.view().unwrap().evidence[0].id, id);
    assert_eq!(restored.view().unwrap().schema_version, 2);
}
#[test]
fn failed_upgrade_rolls_back_version_and_records_and_retains_backup() {
    let (temp, mut w) = workspace();
    w.import("source.txt", b"Synthetic source").unwrap();
    let before = w.revision().unwrap();
    drop(w);
    let db = temp.path().join("case/workspace.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_upgrade BEFORE INSERT ON events WHEN NEW.action='workspace.schema_v2' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    drop(conn);
    assert!(Workspace::open(temp.path().join("case")).is_err());
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row::<u64, _, _>("SELECT revision FROM meta", [], |r| r.get(0))
            .unwrap(),
        before
    );
    assert_eq!(
        std::fs::read_dir(temp.path().join("case/backups"))
            .unwrap()
            .count(),
        1
    );
    conn.execute_batch("DROP TRIGGER reject_upgrade").unwrap();
    drop(conn);
    assert!(Workspace::open(temp.path().join("case")).is_ok());
}

#[test]
fn tab_separated_bom_and_optional_cells_survive_import_and_source_review() {
    let (temp, mut w) = workspace();
    let source = "\u{feff}account\tdate\tposting_date\tdescription\tamount\tcurrency\tbalance\n000003\t2025-04-03\t\t\"Synthetic tab\tinside description\"\t-3.25\tAUD\t\n";
    let sample = sample(source.as_bytes(), Delimiter::Tab).unwrap();
    assert_eq!(sample.headers[0], "account");
    commit(
        &mut w,
        "synthetic.tsv",
        source.as_bytes(),
        sample.suggested_mapping,
        None,
    );
    let view = w.view().unwrap();
    let transaction = &view.transactions[0];
    assert_eq!(transaction.account, "000003");
    assert_eq!(transaction.description, "Synthetic tab\tinside description");
    assert_eq!(transaction.posting_date, None);
    assert_eq!(transaction.balance, None);
    assert_eq!(
        w.inspect_source(&transaction.anchor).unwrap().quote,
        "-3.25"
    );
    assert_eq!(
        std::fs::read(
            temp.path()
                .join("case/originals")
                .join(hash(source.as_bytes()))
        )
        .unwrap(),
        source.as_bytes()
    );
    let observation = ObservationInput {
        entity_id: w
            .add_entity(
                EntityInput {
                    kind: EntityKind::Account,
                    name: "Synthetic account".into(),
                    identifiers: vec![],
                },
                "Synthetic account source",
                w.revision().unwrap(),
            )
            .unwrap(),
        field: "account_reference".into(),
        value: "000003".into(),
        anchor: SourceAnchor::Cell {
            evidence_id: view.evidence[0].id.clone(),
            sheet: "CSV".into(),
            row: 2,
            column: "account".into(),
        },
    };
    w.add_observation(observation, "Mapped source evidence", w.revision().unwrap())
        .unwrap();
}
