//! Desktop report catalogue and explicit immutable-byte retrieval.
use super::*;

// Applies to this in-memory IPC response, not retention or the legacy workspace API.
const MAX_REPORT_RESPONSE_BYTES: u64 = 256 * 1024 * 1024;

impl Workspace {
    pub fn presentation(&self) -> Result<WorkspaceView<ReportMetadata>> {
        self.view_with_reports(report_metadata)
    }

    pub fn inspect_report_snapshot(
        &self,
        key: &str,
        expected_sha256: &str,
    ) -> Result<ReportSnapshot> {
        require(Uuid::parse_str(key).is_ok(), "Invalid report identifier")?;
        require(valid_hash(expected_sha256), "Invalid report digest")?;
        let transaction = self.conn.unchecked_transaction()?;
        // Bound the stored JSON before allocating a Rust copy. Escaped HTML can
        // be larger than the HTML itself; both representations have a limit.
        let stored_bytes: Option<u64> = transaction
            .query_row(
                "SELECT length(CAST(body AS BLOB)) FROM records WHERE kind='report' AND id=?",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        require(
            stored_bytes.is_some_and(|n| n <= MAX_REPORT_RESPONSE_BYTES),
            "Report unavailable or exceeds the 256 MiB in-memory export limit",
        )?;
        let snapshot: ReportSnapshot = get(&transaction, "report", key)?;
        require(
            snapshot.id == key && snapshot.sha256 == expected_sha256,
            "Report identity changed; refresh the report catalogue",
        )?;
        require(
            snapshot.html.len() as u64 <= MAX_REPORT_RESPONSE_BYTES,
            "Report exceeds the 256 MiB in-memory export limit",
        )?;
        require(
            hash(snapshot.html.as_bytes()) == expected_sha256,
            "Retained report failed its integrity check",
        )?;
        transaction.commit()?;
        Ok(snapshot)
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn report_metadata(conn: &Connection) -> Result<Vec<ReportMetadata>> {
    // SQLite projects the metadata without transferring historical HTML into
    // Rust or over IPC. SQLite still reads the JSON record; this is not paging.
    let mut statement = conn.prepare(
        "SELECT id, json_object(
            'id', json_extract(body,'$.id'),
            'workspace_revision', json_extract(body,'$.workspace_revision'),
            'created_at', json_extract(body,'$.created_at'),
            'sha256', json_extract(body,'$.sha256'),
            'html_bytes', CASE WHEN json_type(body,'$.html')='text'
                THEN length(CAST(json_extract(body,'$.html') AS BLOB)) ELSE NULL END
        ) FROM records WHERE kind='report' ORDER BY sequence",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (key, json) = row?;
        let entry: ReportMetadata = serde_json::from_str(&json)?;
        require(
            entry.id == key
                && Uuid::parse_str(&entry.id).is_ok()
                && valid_hash(&entry.sha256)
                && chrono::DateTime::parse_from_rfc3339(&entry.created_at).is_ok(),
            "Invalid retained report metadata",
        )?;
        Ok(entry)
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workspace() -> (tempfile::TempDir, Workspace) {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let workspace = Workspace::open(temp.path().join("case")).unwrap();
        (temp, workspace)
    }
    #[test]
    fn presentation_omits_report_bytes_and_explicit_export_preserves_history() {
        let (_temp, mut w) = workspace();
        w.import("before.txt", "Synthetic café source".as_bytes())
            .unwrap();
        let saved = w.dispatch_presentation(Command::SaveReport {}).unwrap();
        let metadata = &saved["workspace"]["reports"][0];
        assert!(metadata.get("html").is_none());
        let original = w.view().unwrap().reports.remove(0);
        assert_eq!(metadata["html_bytes"], original.html.len());
        assert_eq!(metadata["sha256"], original.sha256);
        let mutated = w
            .dispatch_presentation(Command::Import {
                name: "later.txt".into(),
                bytes: b"Synthetic later correction".to_vec(),
            })
            .unwrap();
        assert_eq!(&mutated["workspace"]["reports"][0], metadata);
        let fetched = w
            .dispatch_presentation(Command::InspectReportSnapshot {
                report_id: original.id.clone(),
                expected_sha256: original.sha256.clone(),
            })
            .unwrap();
        assert_eq!(fetched, serde_json::to_value(&original).unwrap());
        assert_eq!(
            w.dispatch(Command::View {}).unwrap()["workspace"]["reports"][0],
            fetched
        );
        assert_eq!(w.presentation().unwrap().revision, w.revision().unwrap());
    }
    #[test]
    fn selected_export_rejects_missing_wrong_identity_and_corrupt_bytes() {
        let (_temp, mut w) = workspace();
        let id = w.save_report().unwrap();
        let metadata = w.presentation().unwrap().reports.remove(0);
        assert!(w.inspect_report_snapshot(&id, &"a".repeat(64)).is_err());
        assert!(w
            .inspect_report_snapshot(&Uuid::new_v4().to_string(), &metadata.sha256)
            .is_err());
        assert!(w
            .inspect_report_snapshot("../report", &metadata.sha256)
            .is_err());
        w.conn
            .execute(
                "UPDATE records SET body=json_set(body,'$.html','tampered') WHERE kind='report'",
                [],
            )
            .unwrap();
        assert!(w
            .inspect_report_snapshot(&id, &metadata.sha256)
            .unwrap_err()
            .to_string()
            .contains("integrity"));
        assert!(w.conn.is_autocommit());
        w.conn
            .execute(
                "UPDATE records SET body=json_set(body,'$.id','different') WHERE kind='report'",
                [],
            )
            .unwrap();
        assert!(w.presentation().is_err());
        assert!(w.conn.is_autocommit());
    }
    #[test]
    fn metadata_uses_utf8_bytes_and_refuses_non_text_html() {
        let (_temp, mut w) = workspace();
        w.save_report().unwrap();
        w.conn
            .execute(
                "UPDATE records SET body=json_set(body,'$.html',?) WHERE kind='report'",
                ["café 🧭"],
            )
            .unwrap();
        assert_eq!(
            w.presentation().unwrap().reports[0].html_bytes,
            "café 🧭".len() as u64
        );
        w.conn
            .execute(
                "UPDATE records SET body=json_set(body,'$.html',12) WHERE kind='report'",
                [],
            )
            .unwrap();
        assert!(w.presentation().is_err());
    }
}
