//! Canonical report bytes and a private, complete export published without overwriting snapshots.
use super::*;

impl Workspace {
    pub fn save_report(&mut self) -> Result<String> {
        // A new report does not cite prior report HTML. Load its canonical sources
        // in one snapshot without allocating every previous exported document.
        let view = self.view_with_reports(|_| Ok(Vec::<ReportSnapshot>::new()))?;
        for evidence in &view.evidence {
            self.verify_original(evidence)?;
        }
        // A workspace can remain open while its directories change. Validate at
        // the operation boundary, before creating or chmod'ing anything beneath it.
        let exports = self.root.join("exports");
        private_dir(&exports)?;
        let report_id = id();
        let html = report::html(&view, &report_id)?;
        let snapshot = ReportSnapshot {
            id: report_id.clone(),
            workspace_revision: view.revision,
            created_at: now(),
            sha256: hash(html.as_bytes()),
            html,
        };
        let pending = exports.join(format!(".pending-{report_id}.html"));
        let target = exports.join(format!("{report_id}.html"));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&pending)?;
        let mut published = false;
        let result = (|| {
            file.write_all(snapshot.html.as_bytes())?;
            file.sync_all()?;
            // Close before publication: Windows may refuse operations on open files.
            drop(file);
            // Linking a complete file is atomic and refuses an existing destination.
            // Both names are in the same app-owned directory; remove the staging
            // name immediately. Unsupported filesystems fail without a fallback.
            fs::hard_link(&pending, &target)?;
            published = true;
            fs::remove_file(&pending)?;
            self.change(None, "report.snapshot", false, |conn| {
                put(conn, "report", &report_id, &snapshot)
            })?;
            Ok(report_id)
        })();
        if result.is_err() {
            // Only this operation's names are touched. A missing file is already
            // cleaned. Never hide a cleanup failure behind the original failure.
            let mut cleanup_failed = remove_owned_export(&pending).is_err();
            if published {
                cleanup_failed |= remove_owned_export(&target).is_err();
            }
            if cleanup_failed {
                return Err(std::io::Error::other(
                    "Report publication failed and export cleanup requires review",
                )
                .into());
            }
        }
        result
    }
}

fn remove_owned_export(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
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
    fn failed_canonical_publication_removes_only_the_new_export() {
        let (_temp, mut workspace) = workspace();
        workspace
            .import("synthetic.txt", b"Synthetic report source")
            .unwrap();
        let previous = workspace.save_report().unwrap();
        let exports = workspace.root.join("exports");
        let path = exports.join(format!("{previous}.html"));
        let bytes = fs::read(&path).unwrap();
        let before = workspace.view().unwrap();
        workspace.conn.execute_batch("CREATE TRIGGER reject_report BEFORE INSERT ON records WHEN NEW.kind='report' BEGIN SELECT RAISE(ABORT,'synthetic report publication failure'); END;").unwrap();
        assert!(workspace.save_report().is_err());
        let after = workspace.view().unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.reports.len(), 1);
        assert_eq!(fs::read_dir(exports).unwrap().count(), 1);
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(after.reports[0].sha256, hash(&bytes));
        workspace
            .conn
            .execute_batch("DROP TRIGGER reject_report;")
            .unwrap();
        workspace.save_report().unwrap();
        assert_eq!(workspace.view().unwrap().reports.len(), 2);
    }

    #[test]
    fn complete_export_matches_canonical_bytes_and_restored_snapshot() {
        let (temp, mut workspace) = workspace();
        workspace
            .import("synthetic.txt", b"Synthetic source <script>inert</script>")
            .unwrap();
        let key = workspace.save_report().unwrap();
        let view = workspace.view().unwrap();
        let snapshot = &view.reports[0];
        let path = workspace.root.join("exports").join(format!("{key}.html"));
        assert_eq!(fs::read(&path).unwrap(), snapshot.html.as_bytes());
        assert_eq!(hash(snapshot.html.as_bytes()), snapshot.sha256);
        assert!(!snapshot.html.contains("<script>"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let metadata = fs::metadata(&path).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            assert_eq!(metadata.nlink(), 1);
        }
        let backup = workspace.backup().unwrap();
        let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
        assert_eq!(restored.view().unwrap().reports[0].html, snapshot.html);
    }

    #[cfg(unix)]
    #[test]
    fn replaced_export_directory_fails_without_writing_or_chmod_outside() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (temp, mut workspace) = workspace();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o750)).unwrap();
        let exports = workspace.root.join("exports");
        fs::remove_dir(&exports).unwrap();
        symlink(&outside, &exports).unwrap();
        assert!(workspace.save_report().is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(
            fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert!(workspace.view().unwrap().reports.is_empty());
        fs::remove_file(exports).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn replaced_export_junction_fails_without_writing_outside() {
        let (temp, mut workspace) = workspace();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let exports = workspace.root.join("exports");
        fs::remove_dir(&exports).unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&exports)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(status.status.success());
        assert!(workspace.save_report().is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert!(workspace.view().unwrap().reports.is_empty());
        fs::remove_dir(exports).unwrap();
    }
}
