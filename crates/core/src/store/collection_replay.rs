//! Bounded immutable v4 journal capture, outside-lock replay and exact revalidation.
use super::*;
use std::collections::BTreeMap;

const SOURCE_METADATA_BYTES: u64 = 16 * 1024 * 1024;
pub(super) struct Source {
    pub(super) evidence: Evidence,
    pub(super) metadata_sha256: String,
}
/// Owned, bounded canonical input. No live SQLite transaction/connection escapes.
/// This type is neither deserializable nor cloneable.
pub(super) struct CapturedInput {
    root: PathBuf,
    pub(super) raw: String,
    pub(super) job: DurableCollectionJob,
    record_sha256: String,
    pub(super) revision: u64,
    pub(super) sources: BTreeMap<String, Source>,
}

pub(super) struct CapturedRun {
    input: CapturedInput,
    ownership_lifetime: String,
}
// Existing mutation callers can inspect inert input; only capture_run constructs
// the owner-bound wrapper. Read-only captures never acquire this wrapper.
impl std::ops::Deref for CapturedRun {
    type Target = CapturedInput;
    fn deref(&self) -> &Self::Target {
        &self.input
    }
}

pub(super) struct SnapshotCapture(CapturedInput);
pub(super) struct SnapshotPrepared(CapturedInput);
pub(super) fn capture_snapshot(workspace: &Workspace, key: &str) -> Result<SnapshotCapture> {
    capture_input(workspace, key)?
        .map(SnapshotCapture)
        .ok_or_else(|| Error::Validation("Durable snapshot requires a v4 record".into()))
}
impl SnapshotCapture {
    pub(super) fn prepare(self) -> Result<SnapshotPrepared> {
        self.0.replay()?;
        Ok(SnapshotPrepared(self.0))
    }
}
impl SnapshotPrepared {
    pub(super) fn input(&self) -> &CapturedInput {
        &self.0
    }
    pub(super) fn revalidate(&self, workspace: &Workspace, conn: &Connection) -> Result<()> {
        let capture = &self.0;
        require(workspace.root == capture.root, "Snapshot workspace changed")?;
        let revision: u64 = conn.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        require(revision == capture.revision, "Snapshot revision changed")?;
        let (_, raw) = read_job(conn, &capture.job.id)?;
        require(
            hash(raw.as_bytes()) == capture.record_sha256,
            "Snapshot run changed",
        )?;
        capture.validate_sources(conn)
    }
}

/// Consumed replay proof, constructed only from an owned canonical capture.
pub(super) struct PreparedRun {
    pub(super) capture: CapturedRun,
    pub(super) machine: Machine,
}
pub(super) fn capture_run(
    workspace: &Workspace,
    key: &str,
    owner: &CollectionOwnership,
) -> Result<Option<CapturedRun>> {
    workspace.collection_publication_owner(owner)?;
    Ok(capture_input(workspace, key)?.map(|input| CapturedRun {
        input,
        ownership_lifetime: owner.lifetime().into(),
    }))
}
fn capture_input(workspace: &Workspace, key: &str) -> Result<Option<CapturedInput>> {
    let tx = workspace.conn.unchecked_transaction()?;
    let revision = tx.query_row("SELECT revision FROM meta", [], |row| row.get(0))?;
    let (job, raw) = read_job(&tx, key)?;
    if job.schema_version != 4 {
        return Ok(None);
    }
    job.protocol()?;
    require(
        job.events.len() <= MAX_EVENTS
            && canonical_uuid(&job.id)
            && canonical_uuid(&job.request_key),
        "Malformed collection capture",
    )?;
    // Cheap validation only. Full canonical replay occurs after unlocking.
    Machine::new_version(&job.input, job.created_at_ms, 4, true)?;
    let mut references = BTreeMap::new();
    for event in &job.events {
        if let CollectionEvent::TransportObserved { receipt, .. } = event {
            if let FetchRecord::Complete { sha256, bytes, .. } = receipt.fetch_record() {
                valid_response_reference(&sha256, bytes)?;
                if let Some(previous) = references.insert(sha256, bytes) {
                    require(previous == bytes, "Conflicting captured original lengths")?;
                }
            }
        }
    }
    require(
        references.len() <= 50,
        "Collection source capture exceeds request limit",
    )?;
    // Bound all metadata before copying any source body across SQLite.
    let mut metadata_bytes = 0u64;
    for key in references.keys() {
        let bytes: u64 = tx.query_row(
            "SELECT length(CAST(body AS BLOB)) FROM records WHERE kind='evidence' AND id=?",
            [key],
            |row| row.get(0),
        )?;
        require(
            bytes <= MAX_RECORD_BYTES as u64,
            "Response evidence metadata exceeds collection read bound",
        )?;
        metadata_bytes = metadata_bytes
            .checked_add(bytes)
            .ok_or_else(|| Error::Validation("Collection capture size overflow".into()))?;
        require(
            metadata_bytes <= SOURCE_METADATA_BYTES,
            "Collection preparation source metadata exceeds 16 MiB",
        )?;
    }
    let mut sources = BTreeMap::new();
    for (key, bytes) in references {
        let (raw, evidence) = bounded_evidence_raw(&tx, &key)?
            .ok_or_else(|| Error::Validation("Retained response evidence is missing".into()))?;
        require(
            evidence.id == key && evidence.sha256 == key && evidence.bytes == bytes,
            "Captured evidence identity changed",
        )?;
        sources.insert(
            key,
            Source {
                evidence,
                metadata_sha256: hash(raw.as_bytes()),
            },
        );
    }
    Ok(Some(CapturedInput {
        root: workspace.root.clone(),
        raw: raw.clone(),
        job,
        record_sha256: hash(raw.as_bytes()),
        revision,
        sources,
    }))
}
impl CapturedRun {
    pub(super) fn prepare(self) -> Result<PreparedRun> {
        let machine = self.input.replay()?;
        Ok(PreparedRun {
            capture: self,
            machine,
        })
    }
}
impl CapturedInput {
    fn replay(&self) -> Result<Machine> {
        collection_machine::replay(&self.job, |request, result| {
            if let FetchRecord::Complete { sha256, bytes, .. } = result {
                let source = self.sources.get(sha256).ok_or_else(|| {
                    Error::Validation("Original absent from collection capture".into())
                })?;
                validate_acquisition(&self.job, request, sha256, *bytes, &source.evidence)?;
                // Read one original at a time; never retain the full body history.
                Ok(Some(read_original(&self.root, &source.evidence)?))
            } else {
                Ok(None)
            }
        })
    }
    fn validate_sources(&self, conn: &Connection) -> Result<()> {
        for (key, source) in &self.sources {
            let (raw, _) = bounded_evidence_raw(conn, key)?
                .ok_or_else(|| Error::Validation("Captured source disappeared".into()))?;
            require(
                hash(raw.as_bytes()) == source.metadata_sha256,
                "Captured source metadata changed; prepare collection again",
            )?;
            read_original(&self.root, &source.evidence)?;
        }
        Ok(())
    }
}
impl PreparedRun {
    pub(super) fn revalidate(
        self,
        workspace: &Workspace,
        owner: &CollectionOwnership,
    ) -> Result<(Loaded, bool)> {
        workspace.collection_publication_owner(owner)?;
        let Self {
            capture,
            machine: before,
        } = self;
        require(
            workspace.root == capture.root && owner.lifetime() == capture.ownership_lifetime,
            "Prepared collection ownership or workspace changed",
        )?;
        let tx = workspace.conn.unchecked_transaction()?;
        let revision = tx.query_row("SELECT revision FROM meta", [], |row| row.get(0))?;
        let (current, raw) = read_job(&tx, &capture.job.id)?;
        // An unrelated canonical revision may advance. Every captured source row
        // must still be exact; originals are rehashed through the bounded reader.
        capture.input.validate_sources(&tx)?;
        let exact = hash(raw.as_bytes()) == capture.record_sha256;
        let mut loaded = Loaded {
            job: capture.input.job,
            machine: before,
            revision,
        };
        if !exact {
            // The ONLY accepted run drift is one appended canonical Cancel.
            // Reapply the existing rule and require the WHOLE record to match;
            // no caller-controlled checkpoint/frontier or arbitrary suffix wins.
            require(
                current.events.len() == loaded.job.events.len() + 1
                    && current.events[..loaded.job.events.len()] == loaded.job.events,
                "Prepared collection run changed; prepare collection again",
            )?;
            let event = current.events.last().expect("one appended event").clone();
            require(
                matches!(event, CollectionEvent::Cancel { .. }),
                "Prepared collection run has a non-cancellation suffix",
            )?;
            require(
                loaded.job.events.len() < MAX_EVENTS,
                "Collection event bound reached",
            )?;
            loaded.machine.replay_cancel_suffix(&event)?;
            loaded.job.events.push(event);
            loaded.job.checkpoint = loaded.machine.checkpoint.clone();
            bounded(&loaded.job)?;
            require(
                loaded.job == current,
                "Prepared cancellation checkpoint changed",
            )?;
        }
        drop(tx);
        Ok((loaded, exact))
    }
}
