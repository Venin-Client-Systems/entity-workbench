//! Bounded immutable v4 journal capture, outside-lock replay and exact revalidation.
use super::*;
use std::collections::BTreeMap;

const SOURCE_METADATA_BYTES: u64 = 16 * 1024 * 1024;
struct Source {
    evidence: Evidence,
    metadata_sha256: String,
}
/// Owned, bounded canonical input. No live SQLite transaction/connection escapes.
/// This type is neither deserializable nor cloneable.
pub(super) struct CapturedRun {
    root: PathBuf,
    ownership_lifetime: String,
    pub(super) job: DurableCollectionJob,
    record_sha256: String,
    pub(super) revision: u64,
    sources: BTreeMap<String, Source>,
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
    Ok(Some(CapturedRun {
        root: workspace.root.clone(),
        ownership_lifetime: owner.lifetime().into(),
        job,
        record_sha256: hash(raw.as_bytes()),
        revision,
        sources,
    }))
}
impl CapturedRun {
    pub(super) fn prepare(self) -> Result<PreparedRun> {
        let machine = collection_machine::replay(&self.job, |request, result| {
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
        })?;
        Ok(PreparedRun {
            capture: self,
            machine,
        })
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
        for (key, source) in &capture.sources {
            let (raw, _) = bounded_evidence_raw(&tx, key)?
                .ok_or_else(|| Error::Validation("Captured source disappeared".into()))?;
            require(
                hash(raw.as_bytes()) == source.metadata_sha256,
                "Captured source metadata changed; prepare collection again",
            )?;
            read_original(&workspace.root, &source.evidence)?;
        }
        let exact = hash(raw.as_bytes()) == capture.record_sha256;
        let mut loaded = Loaded {
            job: capture.job,
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
