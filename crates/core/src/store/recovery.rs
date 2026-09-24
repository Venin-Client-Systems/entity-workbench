//! Backup references come from one SQLite snapshot, not a sequence of live queries.
use super::*;
use crate::processing::{DerivativeRef, ImageRegionExtractionRecord};
use std::{collections::BTreeMap, io::Read};

impl Workspace {
    pub(super) fn derivative_refs(&self) -> Result<Vec<DerivativeRef>> {
        let records: Vec<ImageRegionExtractionRecord> = all(&self.conn, "image_region_extraction")?;
        let version: u32 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 4 {
            require(
                records.is_empty(),
                "Older workspace cannot contain retained word-region records",
            )?;
            return Ok(Vec::new());
        }
        let mut references = BTreeMap::new();
        for record in records {
            let inspected = self.inspect_image_region_extraction(&record.id)?;
            for reference in processing_regions::refs(&inspected.extraction) {
                if let Some(previous) =
                    references.insert(reference.sha256.clone(), reference.clone())
                {
                    require(
                        previous.bytes == reference.bytes,
                        "Conflicting derivative byte lengths",
                    )?;
                }
            }
        }
        let mut statement = self
            .conn
            .prepare("SELECT sha256,bytes FROM derivative_objects ORDER BY sha256")?;
        let catalog = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
        let expected: BTreeMap<_, _> = references
            .iter()
            .map(|(sha, r)| (sha.clone(), r.bytes))
            .collect();
        require(
            catalog == expected,
            "Derivative catalog does not match canonical references",
        )?;
        Ok(references.into_values().collect())
    }

    pub fn backup(&mut self) -> Result<PathBuf> {
        self.backup_snapshot(|| Ok(()))
    }
    pub(super) fn backup_snapshot(
        &mut self,
        after_snapshot: impl FnOnce() -> Result<()>,
    ) -> Result<PathBuf> {
        let path = self.root.join("backups").join(id());
        private_dir(&path)?;
        let snapshot = path.join("workspace.db");
        self.conn.backup("main", &snapshot, None)?;
        private_file(&snapshot, 0o600)?;
        after_snapshot()?;
        let source = Self {
            root: self.root.clone(),
            runtime: None,
            conn: Connection::open_with_flags(
                &snapshot,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?,
        };
        let evidence = all_evidence(&source.conn)?;
        let derivatives = source.derivative_refs()?;
        copy_sources(&source.root, &path, &evidence, &derivatives)?;
        let copied = Self {
            root: path.clone(),
            runtime: None,
            conn: Connection::open_with_flags(
                &snapshot,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?,
        };
        for original in &evidence {
            copied.verify_original(original)?;
        }
        copied.derivative_refs()?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&snapshot)?
            .sync_all()?;
        let manifest = json!({
            "format_version":2,"complete":true,
            "schema_version":source.conn.pragma_query_value::<u32,_>(None,"user_version",|r|r.get(0))?,
            "revision":source.revision()?,
            "evidence":evidence.iter().map(|e|&e.sha256).collect::<Vec<_>>(),
            "derivatives":derivatives.iter().map(|r|json!({"sha256":r.sha256,"bytes":r.bytes})).collect::<Vec<_>>()
        });
        let manifest_path = path.join("manifest.json");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest_path)?;
        private_file(&manifest_path, 0o600)?;
        file.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        file.sync_all()?;
        Ok(path)
    }

    pub fn restore(backup: &Path, destination: &Path) -> Result<Self> {
        require(!destination.exists(), "Restore destination must not exist")?;
        reject_link_ancestors(backup)?;
        reject_link_ancestors(&backup.join("workspace.db"))?;
        let source = Self {
            root: backup.to_path_buf(),
            runtime: None,
            conn: Connection::open_with_flags(
                backup.join("workspace.db"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?,
        };
        let version: u32 = source
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        require(
            (1..=SCHEMA).contains(&version),
            "Backup schema is unsupported",
        )?;
        let manifest_path = backup.join("manifest.json");
        reject_link_ancestors(&manifest_path)?;
        let metadata = fs::symlink_metadata(&manifest_path)?;
        require(
            metadata.is_file() && !is_link(&metadata) && metadata.len() <= 16 * 1024 * 1024,
            "Backup has no bounded completion manifest",
        )?;
        let mut manifest_bytes = Vec::new();
        fs::File::open(&manifest_path)?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut manifest_bytes)?;
        require(
            manifest_bytes.len() <= 16 * 1024 * 1024,
            "Backup manifest exceeds its bound",
        )?;
        reject_link_ancestors(&manifest_path)?;
        let manifest: Value = serde_json::from_slice(&manifest_bytes)?;
        require(
            manifest["schema_version"] == version && manifest["revision"] == source.revision()?,
            "Backup manifest does not match its snapshot",
        )?;
        if version >= 4 {
            require(
                manifest["format_version"] == 2 && manifest["complete"] == true,
                "Derivative backup is incomplete",
            )?;
        }
        let evidence = all_evidence(&source.conn)?;
        for e in &evidence {
            source.verify_original(e)?;
        }
        let derivatives = source.derivative_refs()?;
        require(
            manifest["evidence"] == json!(evidence.iter().map(|e| &e.sha256).collect::<Vec<_>>()),
            "Backup evidence manifest differs from snapshot",
        )?;
        if version >= 4 {
            require(
                manifest["derivatives"]
                    == json!(derivatives
                        .iter()
                        .map(|r| json!({"sha256":r.sha256,"bytes":r.bytes}))
                        .collect::<Vec<_>>()),
                "Backup derivative manifest differs from snapshot",
            )?;
        }
        // Originals and derivative files are ready before a canonical database name appears.
        private_dir(destination)?;
        copy_sources(backup, destination, &evidence, &derivatives)?;
        let pending = destination.join(format!(".restore-{}.db", id()));
        source.conn.backup("main", &pending, None)?;
        private_file(&pending, 0o600)?;
        let copied = Self {
            root: destination.to_path_buf(),
            runtime: None,
            conn: Connection::open_with_flags(
                &pending,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?,
        };
        for e in &evidence {
            copied.verify_original(e)?;
        }
        copied.derivative_refs()?;
        drop(copied);
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pending)?
            .sync_all()?;
        reject_link_ancestors(&destination.join("workspace.db"))?;
        fs::hard_link(&pending, destination.join("workspace.db"))?;
        fs::remove_file(pending)?;
        Self::open(destination)
    }
}

fn copy_sources(
    source: &Path,
    destination: &Path,
    evidence: &[Evidence],
    derivatives: &[DerivativeRef],
) -> Result<()> {
    private_dir(&destination.join("originals"))?;
    for e in evidence {
        let bytes = read_original(source, e)?;
        let target = destination.join("originals").join(&e.sha256);
        reject_link_ancestors(&target)?;
        require(
            !target.exists(),
            "Backup original destination already exists",
        )?;
        originals::write_verified_copy(&target, e, &bytes)?;
    }
    for reference in derivatives {
        let bytes = derivative_files::read(source, reference)?;
        derivative_files::retain(destination, reference, &bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_uses_snapshot_references_after_another_canonical_writer_commits() {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let root = temp.path().join("case");
        let mut workspace = Workspace::open(&root).unwrap();
        let first = workspace
            .import("first.txt", b"First synthetic source")
            .unwrap();
        let revision = workspace.revision().unwrap();
        let backup = workspace
            .backup_snapshot(|| {
                let mut other = Workspace::open(&root)?;
                other.import("later.txt", b"Later synthetic source")?;
                Ok(())
            })
            .unwrap();
        assert_eq!(workspace.view().unwrap().evidence.len(), 2);
        let manifest: Value =
            serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["revision"], revision);
        assert_eq!(manifest["evidence"], json!([first]));
        let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
        assert_eq!(restored.view().unwrap().evidence.len(), 1);
        assert_eq!(restored.revision().unwrap(), revision);
        assert_eq!(fs::read_dir(backup.join("originals")).unwrap().count(), 1);
    }
    #[test]
    fn schema_three_upgrade_backs_up_and_failed_upgrade_rolls_back_catalog() {
        for fail in [false, true] {
            let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
            let root = temp.path().join("case");
            let mut workspace = Workspace::open(&root).unwrap();
            let source = workspace
                .import("source.txt", b"Synthetic original")
                .unwrap();
            let revision = workspace.revision().unwrap();
            workspace
                .conn
                .execute_batch("DROP TABLE derivative_objects; PRAGMA user_version=3;")
                .unwrap();
            if fail {
                workspace.conn.execute_batch("CREATE TRIGGER reject_upgrade BEFORE INSERT ON events WHEN NEW.action='workspace.schema_v4' BEGIN SELECT RAISE(ABORT,'synthetic upgrade failure'); END;").unwrap();
            }
            drop(workspace);
            let upgraded = Workspace::open(&root);
            assert_eq!(upgraded.is_err(), fail);
            let check = Connection::open(root.join("workspace.db")).unwrap();
            let version: u32 = check
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(version, if fail { 3 } else { 4 });
            assert_eq!(
                check
                    .query_row::<u32, _, _>(
                        "SELECT count(*) FROM sqlite_master WHERE name='derivative_objects'",
                        [],
                        |r| r.get(0)
                    )
                    .unwrap(),
                u32::from(!fail)
            );
            assert_eq!(
                check
                    .query_row::<u64, _, _>("SELECT revision FROM meta", [], |r| r.get(0))
                    .unwrap(),
                revision + u64::from(!fail)
            );
            let backup = fs::read_dir(root.join("backups"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let snapshot = Connection::open(backup.join("workspace.db")).unwrap();
            assert_eq!(
                snapshot
                    .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                    .unwrap(),
                3
            );
            if fail {
                snapshot
                    .execute_batch("DROP TRIGGER reject_upgrade")
                    .unwrap();
            }
            drop(snapshot);
            // A historical schema-3 backup had only these three manifest fields.
            let manifest_path = backup.join("manifest.json");
            let mut manifest: Value =
                serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
            for key in ["format_version", "complete", "derivatives"] {
                manifest.as_object_mut().unwrap().remove(key);
            }
            fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
            assert_eq!(restored.view().unwrap().schema_version, 4);
            assert_eq!(restored.view().unwrap().evidence[0].id, source);
            assert_eq!(
                fs::read(temp.path().join("restored/originals").join(source)).unwrap(),
                b"Synthetic original"
            );
        }
    }
    #[test]
    fn copy_sources_preserves_existing_destination_and_rejects_corrupt_source() {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut w = Workspace::open(temp.path().join("source")).unwrap();
        let key = w
            .import("synthetic.txt", b"Original copy specimen")
            .unwrap();
        let e: Evidence = get(&w.conn, "evidence", &key).unwrap();
        let destination = temp.path().join("destination");
        private_dir(&destination.join("originals")).unwrap();
        let target = destination.join("originals").join(&key);
        fs::write(&target, b"preserve existing").unwrap();
        assert!(copy_sources(&w.root, &destination, std::slice::from_ref(&e), &[]).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"preserve existing");
        fs::remove_file(&target).unwrap();
        let original = w.root.join("originals").join(&key);
        fs::remove_file(&original).unwrap();
        fs::write(&original, b"Altered copy specimen!").unwrap();
        assert!(copy_sources(&w.root, &destination, &[e], &[]).is_err());
        assert!(!target.exists());
    }
    #[test]
    fn corrupt_backup_original_cannot_publish_restored_database() {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut w = Workspace::open(temp.path().join("source")).unwrap();
        let key = w
            .import("synthetic.txt", b"Original backup specimen")
            .unwrap();
        let backup = w.backup().unwrap();
        let original = backup.join("originals").join(&key);
        fs::remove_file(&original).unwrap();
        fs::write(&original, b"Altered! backup specimen").unwrap();
        let destination = temp.path().join("restore");
        assert!(Workspace::restore(&backup, &destination).is_err());
        assert!(!destination.join("workspace.db").exists());
        assert!(!destination.exists());
        assert_eq!(
            fs::read(w.root.join("originals").join(&key)).unwrap(),
            b"Original backup specimen"
        );
    }
    #[cfg(unix)]
    #[test]
    fn linked_backup_source_is_refused_before_any_original_copy() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut w = Workspace::open(temp.path().join("source")).unwrap();
        let key = w
            .import("synthetic.txt", b"Linked backup specimen")
            .unwrap();
        let e: Evidence = get(&w.conn, "evidence", &key).unwrap();
        let outside = temp.path().join("outside");
        fs::write(&outside, b"Linked backup specimen").unwrap();
        let original = w.root.join("originals").join(&key);
        fs::remove_file(&original).unwrap();
        symlink(&outside, &original).unwrap();
        let destination = temp.path().join("copy");
        assert!(copy_sources(&w.root, &destination, &[e], &[]).is_err());
        assert!(!destination.join("originals").join(key).exists());
        assert_eq!(fs::read(&outside).unwrap(), b"Linked backup specimen");
    }
}
