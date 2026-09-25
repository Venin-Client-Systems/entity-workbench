use super::*;

fn workspace() -> (tempfile::TempDir, Workspace, String) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    let id = w
        .import(
            "synthetic-source.txt",
            b"Retained synthetic source.\nSecond exact line.\n",
        )
        .unwrap();
    (temp, w, id)
}
fn anchor(id: &str) -> SourceAnchor {
    SourceAnchor::Text {
        evidence_id: id.into(),
        line_start: 2,
        line_end: 2,
    }
}
#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)]
fn writable(path: &Path) {
    // Deliberately corrupt only the test-owned immutable evidence fixture.
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
}
#[cfg(unix)]
fn writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn altered_missing_and_restored_originals_control_excerpt_availability() {
    let (_temp, w, key) = workspace();
    let e: Evidence = get(&w.conn, "evidence", &key).unwrap();
    let path = w.root.join("originals").join(&e.sha256);
    let original = fs::read(&path).unwrap();
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let selected = anchor(&key);
    assert_eq!(
        w.inspect_source(&selected).unwrap().quote,
        "Second exact line.\n"
    );
    writable(&path);
    let mut altered = original.clone();
    altered[0] = b'X';
    fs::write(&path, &altered).unwrap();
    assert!(w
        .inspect_source(&selected)
        .unwrap_err()
        .to_string()
        .contains("checksum mismatch"));
    assert!(w.conn.is_autocommit());
    fs::write(&path, &original).unwrap();
    assert_eq!(
        w.inspect_source(&selected).unwrap().quote,
        "Second exact line.\n"
    );
    fs::remove_file(&path).unwrap();
    assert!(w.inspect_source(&selected).is_err());
    fs::write(&path, &original).unwrap();
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
    assert!(w.conn.is_autocommit());
}

#[test]
fn valid_original_does_not_hide_canonical_evidence_identity_corruption() {
    let (_temp, w, key) = workspace();
    w.conn.execute("UPDATE records SET body=json_set(body,'$.id','different-source') WHERE kind='evidence' AND id=?", [&key]).unwrap();
    assert!(w
        .inspect_source(&anchor(&key))
        .unwrap_err()
        .to_string()
        .contains("Canonical source identity"));
    assert!(w.conn.is_autocommit());
}

#[test]
fn retargeting_an_evidence_digest_cannot_verify_a_different_original() {
    let (_temp, mut w, key) = workspace();
    let second = w
        .import("second-synthetic.txt", b"A different preserved original.\n")
        .unwrap();
    let source: Evidence = get(&w.conn, "evidence", &key).unwrap();
    let substitute: Evidence = get(&w.conn, "evidence", &second).unwrap();
    // Preserve A's key, body ID and derivative, while maliciously repointing
    // its digest/length to intact original B. A must not quote B as its proof.
    w.conn.execute("UPDATE records SET body=json_set(body,'$.sha256',?1,'$.bytes',?2) WHERE kind='evidence' AND id=?3", params![substitute.sha256, substitute.bytes, key]).unwrap();
    let corrupted: Evidence = get(&w.conn, "evidence", &key).unwrap();
    assert!(w
        .verify_original(&corrupted)
        .unwrap_err()
        .to_string()
        .contains("identity does not match its original digest"));
    let original = w.root.join("originals").join(source.sha256);
    writable(&original);
    fs::remove_file(original).unwrap();
    assert!(w
        .inspect_source(&anchor(&key))
        .unwrap_err()
        .to_string()
        .contains("Canonical source identity"));
    assert!(w.conn.is_autocommit());
}

#[test]
fn cell_excerpt_retains_original_decimal_text_and_refuses_changed_statement() {
    let (_temp, mut w, _key) = workspace();
    let key = w.import("synthetic.csv", b"account,date,description,amount,currency\n0042,2025-01-01,Synthetic purchase,-0.10000001,AUD\n").unwrap();
    let e: Evidence = get(&w.conn, "evidence", &key).unwrap();
    let selected = SourceAnchor::Cell {
        evidence_id: key,
        sheet: "CSV".into(),
        row: 2,
        column: "amount".into(),
    };
    let result = w
        .dispatch(Command::InspectSource {
            anchor: selected.clone(),
        })
        .unwrap();
    assert_eq!(result["quote"], "-0.10000001");
    assert_eq!(result["workspace_revision"], w.revision().unwrap());
    let path = w.root.join("originals").join(e.sha256);
    writable(&path);
    fs::write(&path, b"changed synthetic statement").unwrap();
    assert!(w
        .dispatch_presentation(Command::InspectSource { anchor: selected })
        .unwrap_err()
        .to_string()
        .contains("altered original"));
}
