//! Store-backed synchronous read/export. Does not acquire execution ownership or write SQLite.
use super::replay::{capture_snapshot, SnapshotPrepared};
use super::*;
use crate::collection_snapshot::*;
use std::collections::BTreeSet;

#[path = "collection_bundle_io.rs"]
mod io;

struct Count(usize);
impl std::io::Write for Count {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|n| *n <= SNAPSHOT_BYTES)
            .ok_or_else(|| std::io::Error::other("Collection snapshot exceeds 16 MiB"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut count = Count(0);
    serde_json::to_writer(&mut count, value)?;
    let mut bytes = Vec::with_capacity(count.0);
    serde_json::to_writer(&mut bytes, value)?;
    Ok(bytes)
}
fn snapshot(prepared: &SnapshotPrepared) -> Result<DurableCollectionSnapshot> {
    let input = prepared.input();
    let result = DurableCollectionSnapshot {
        schema_version: 1,
        workspace_revision: input.revision,
        canonical_run_sha256: hash(input.raw.as_bytes()),
        canonical_run_bytes: input.raw.len() as u64,
        canonical_run_json: input.raw.clone(),
        inspection: public_api::project_inspection(&input.job, input.revision)?,
        sources: input
            .sources
            .iter()
            .map(|(sha, source)| SnapshotSource {
                evidence_id: sha.clone(),
                sha256: sha.clone(),
                bytes: source.evidence.bytes,
                canonical_metadata_sha256: source.metadata_sha256.clone(),
                acquisitions: source
                    .evidence
                    .acquisitions
                    .iter()
                    .filter(|a| a.job_id == input.job.id)
                    .cloned()
                    .collect(),
            })
            .collect(),
    };
    encode(&result)?;
    Ok(result)
}
fn file(path: String, bytes: &[u8]) -> CollectionExportFile {
    CollectionExportFile {
        path,
        sha256: hash(bytes),
        bytes: bytes.len() as u64,
    }
}
fn path(root: &Path, export_id: &str) -> Result<PathBuf> {
    require(
        canonical_uuid(export_id),
        "Invalid collection export identifier",
    )?;
    Ok(root.join("exports").join(format!("durable-{export_id}")))
}
impl Workspace {
    pub fn durable_collection_snapshot(&self, job_id: &str) -> Result<DurableCollectionSnapshot> {
        let prepared = capture_snapshot(self, job_id)?.prepare()?;
        let result = snapshot(&prepared)?;
        let tx = self.conn.unchecked_transaction()?;
        prepared.revalidate(self, &tx)?;
        Ok(result)
    }
    pub fn export_durable_collection(
        &self,
        job_id: &str,
        export_id: &str,
    ) -> Result<DurableCollectionExport> {
        self.export_durable_collection_with(job_id, export_id, || Ok(()))
    }
    fn export_durable_collection_with(
        &self,
        job_id: &str,
        export_id: &str,
        before_publish: impl FnOnce() -> Result<()>,
    ) -> Result<DurableCollectionExport> {
        self.export_durable_collection_with_marker(job_id, export_id, before_publish, || Ok(()))
    }
    fn export_durable_collection_with_marker(
        &self,
        job_id: &str,
        export_id: &str,
        before_publish: impl FnOnce() -> Result<()>,
        before_marker: impl FnOnce() -> Result<()>,
    ) -> Result<DurableCollectionExport> {
        let destination = path(&self.root, export_id)?;
        let prepared = capture_snapshot(self, job_id)?.prepare()?;
        let snapshot = snapshot(&prepared)?;
        let bytes = encode(&snapshot)?;
        let mut owned = io::OwnedBundle::create(&destination)?;
        let mut completion_started = false;
        let result = (|| {
            owned.directory("originals")?;
            owned.write("snapshot.json", &bytes)?;
            let mut originals = Vec::new();
            let mut total = 0u64;
            for (sha, source) in &prepared.input().sources {
                total = total
                    .checked_add(source.evidence.bytes)
                    .ok_or_else(|| Error::Validation("Collection original size overflow".into()))?;
                require(
                    total <= ORIGINAL_BYTES,
                    "Collection originals exceed 100 MiB",
                )?;
                let body = read_original(&self.root, &source.evidence)?;
                let relative = format!("originals/{sha}.bin");
                owned.write(&relative, &body)?;
                originals.push(file(relative, &body));
            }
            let receipt = DurableCollectionExport {
                schema_version: 1,
                export_id: export_id.into(),
                job_id: job_id.into(),
                workspace_revision: snapshot.workspace_revision,
                canonical_run_sha256: snapshot.canonical_run_sha256.clone(),
                snapshot: file("snapshot.json".into(), &bytes),
                originals,
            };
            before_publish()?;
            // No canonical writes, but exclude SQLite writers from the final exact
            // validation through marker publication (also in WAL mode). Filesystem
            // identity checks are separate; this is not a hostile-process sandbox.
            let tx = rusqlite::Transaction::new_unchecked(
                &self.conn,
                rusqlite::TransactionBehavior::Immediate,
            )?;
            prepared.revalidate(self, &tx)?;
            owned.verify()?;
            owned.sync()?;
            let marker = encode(&receipt)?;
            require(marker.len() <= 64 * 1024, "Completion marker exceeds bound")?;
            before_marker()?;
            completion_started = true;
            owned.write("complete.json", &marker)?;
            owned.verify()?;
            owned.sync()?;
            Ok(receipt)
        })();
        match result {
            Ok(receipt) => Ok(receipt),
            Err(error) if completion_started => Err(Error::Cleanup(format!("Collection export completion is unconfirmed ({}); inspect the export identifier without retrying publication", category(&error)))),
            Err(error) => match owned.cleanup() {
                Ok(()) => Err(error),
                Err(_) => Err(Error::Cleanup(format!("Collection export failed ({}) and owned cleanup could not be verified; inspect the export identifier", category(&error)))),
            }
        }
    }
    pub fn inspect_durable_collection_export(
        &self,
        export_id: &str,
    ) -> Result<DurableCollectionExportInspection> {
        let root = path(&self.root, export_id)?;
        let bundle = io::Bundle::open(&root)?;
        let marker = bundle.read("complete.json", 64 * 1024)?;
        let receipt: DurableCollectionExport = serde_json::from_slice(&marker)?;
        require(
            receipt.schema_version == 1
                && receipt.export_id == export_id
                && canonical_uuid(&receipt.job_id)
                && receipt.snapshot.path == "snapshot.json"
                && receipt.snapshot.bytes <= SNAPSHOT_BYTES as u64
                && receipt.originals.len() <= 50,
            "Invalid collection completion marker",
        )?;
        let bytes = bundle.read(&receipt.snapshot.path, SNAPSHOT_BYTES)?;
        require(
            file(receipt.snapshot.path.clone(), &bytes) == receipt.snapshot,
            "Exported snapshot identity changed",
        )?;
        let snapshot: DurableCollectionSnapshot = serde_json::from_slice(&bytes)?;
        require(
            snapshot.schema_version == 1
                && snapshot.workspace_revision == receipt.workspace_revision
                && snapshot.canonical_run_sha256 == receipt.canonical_run_sha256
                && snapshot.canonical_run_bytes == snapshot.canonical_run_json.len() as u64
                && snapshot.canonical_run_bytes <= MAX_RECORD_BYTES as u64
                && hash(snapshot.canonical_run_json.as_bytes()) == snapshot.canonical_run_sha256,
            "Invalid frozen collection snapshot binding",
        )?;
        let job: DurableCollectionJob = serde_json::from_str(&snapshot.canonical_run_json)?;
        job.protocol()?;
        require(
            job.schema_version == 4
                && job.id == receipt.job_id
                && canonical_uuid(&job.id)
                && canonical_uuid(&job.request_key)
                && job.events.len() <= MAX_EVENTS
                && snapshot.sources.len() <= 50,
            "Invalid frozen collection run",
        )?;
        let expected = public_api::project_inspection(&job, snapshot.workspace_revision)?;
        require(
            serde_json::to_vec(&expected)? == serde_json::to_vec(&snapshot.inspection)?,
            "Frozen inspection differs from run",
        )?;
        let mut seen = BTreeSet::new();
        let mut total = 0u64;
        let mut inventory = vec!["complete.json".to_owned(), "snapshot.json".to_owned()];
        require(
            receipt.originals.len() == snapshot.sources.len(),
            "Original inventory is incomplete",
        )?;
        require(
            snapshot
                .sources
                .windows(2)
                .all(|pair| pair[0].sha256 < pair[1].sha256),
            "Original inventory order differs",
        )?;
        for (source, item) in snapshot.sources.iter().zip(&receipt.originals) {
            valid_response_reference(&source.sha256, source.bytes)?;
            require(
                source.evidence_id == source.sha256
                    && seen.insert(source.sha256.clone())
                    && source.canonical_metadata_sha256.len() == 64
                    && source
                        .canonical_metadata_sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && source.acquisitions.iter().all(|a| a.job_id == job.id)
                    && item.path == format!("originals/{}.bin", source.sha256)
                    && item.sha256 == source.sha256
                    && item.bytes == source.bytes,
                "Invalid exported original identity",
            )?;
            total = total
                .checked_add(source.bytes)
                .ok_or_else(|| Error::Validation("Original inventory overflow".into()))?;
            require(
                total <= ORIGINAL_BYTES,
                "Collection originals exceed 100 MiB",
            )?;
            inventory.push(item.path.clone());
        }
        let mut used = BTreeSet::new();
        collection_machine::replay(&job, |request, result| {
            if let FetchRecord::Complete { sha256, bytes, .. } = result {
                let source = snapshot
                    .sources
                    .iter()
                    .find(|s| s.sha256 == *sha256)
                    .ok_or_else(|| Error::Validation("Exported original is missing".into()))?;
                let evidence = Evidence {
                    id: source.evidence_id.clone(),
                    sha256: sha256.clone(),
                    bytes: *bytes,
                    name: String::new(),
                    media_type: String::new(),
                    origin_group: sha256.clone(),
                    imported_at: String::new(),
                    extraction_status: String::new(),
                    text: None,
                    acquisitions: source.acquisitions.clone(),
                };
                require(source.bytes == *bytes, "Frozen original size differs")?;
                validate_acquisition(&job, request, sha256, *bytes, &evidence)?;
                let body = bundle.read(&format!("originals/{sha256}.bin"), 2 * 1024 * 1024)?;
                require(
                    body.len() as u64 == *bytes && hash(&body) == *sha256,
                    "Exported original checksum differs",
                )?;
                used.insert(sha256.clone());
                Ok(Some(body))
            } else {
                Ok(None)
            }
        })?;
        require(used == seen, "Export contains unreferenced originals")?;
        bundle.inventory(&inventory)?;
        Ok(DurableCollectionExportInspection { receipt, snapshot })
    }
}
fn category(error: &Error) -> &'static str {
    match error {
        Error::Io(_) => "filesystem",
        Error::Database(_) => "database",
        Error::Json(_) => "json",
        Error::Blocked(_) => "blocked",
        Error::Conflict(_) => "conflict",
        _ => "validation",
    }
}
#[cfg(test)]
#[path = "collection_snapshot_tests.rs"]
mod tests;
