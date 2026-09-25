//! One replay-verified v4 capture authorizes an explicit cancellation, not execution.
use super::replay::{capture_run, CapturedRun, PreparedRun};
use super::*;

pub(crate) struct CollectionCancellationCapture {
    run: CapturedRun,
    generation: u32,
    protocol: CollectionProtocol,
}
pub(crate) struct PreparedCollectionCancellation {
    run: PreparedRun,
    generation: u32,
    protocol: CollectionProtocol,
}
/// Private consumed publication candidate. Callers must preflight the acknowledgement
/// before committing; no replay or DTO construction is needed after the write.
pub(crate) struct ReadyCollectionCancellation {
    loaded: Loaded,
    changed: bool,
    ownership_lifetime: String,
    root: PathBuf,
}
impl CollectionCancellationCapture {
    pub(crate) fn prepare(self) -> Result<PreparedCollectionCancellation> {
        Ok(PreparedCollectionCancellation {
            run: self.run.prepare()?,
            generation: self.generation,
            protocol: self.protocol,
        })
    }
}
impl ReadyCollectionCancellation {
    pub(crate) fn inspection(&self) -> Result<crate::collection_api::CollectionRunInspection> {
        let revision = if self.changed {
            self.loaded
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::Validation("Collection revision overflow".into()))?
        } else {
            self.loaded.revision
        };
        public_api::project_inspection(&self.loaded.job, revision)
    }
}
fn cancellable(
    job: &DurableCollectionJob,
    generation: u32,
    protocol: CollectionProtocol,
) -> Result<()> {
    require(
        protocol.version() == 4 && protocol.matches(job),
        "Attached collection cancellation policy or mode changed",
    )?;
    require(
        job.checkpoint.generation == generation,
        "Collection generation changed",
    )?;
    require(
        job.checkpoint.cancellation_requested
            || matches!(
                job.checkpoint.state,
                CollectionState::Queued | CollectionState::Running | CollectionState::Interrupted
            ),
        "Collection cancellation is unavailable",
    )
}
impl Workspace {
    pub(crate) fn capture_collection_cancellation(
        &self,
        id: &str,
        generation: u32,
        owner: &CollectionOwnership,
        protocol: CollectionProtocol,
    ) -> Result<CollectionCancellationCapture> {
        self.collection_owner(owner)?;
        self.collection_transport_available()?;
        let run = capture_run(self, id, owner)?.ok_or_else(|| {
            Error::Validation("Prepared cancellation requires attached v4 policy".into())
        })?;
        cancellable(&run.job, generation, protocol)?;
        Ok(CollectionCancellationCapture {
            run,
            generation,
            protocol,
        })
    }
    pub(crate) fn ready_collection_cancellation(
        &self,
        prepared: PreparedCollectionCancellation,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<ReadyCollectionCancellation> {
        self.collection_owner(owner)?;
        let (mut loaded, _) = prepared.run.revalidate(self, owner)?;
        self.collection_transport_available()?;
        cancellable(&loaded.job, prepared.generation, prepared.protocol)?;
        let changed = !loaded.job.checkpoint.cancellation_requested;
        if changed {
            // Fresh public intent keeps strict new-event time rules. Only a Cancel
            // already in canonical history uses the shared historical suffix rule.
            append(&mut loaded, CollectionEvent::Cancel { at_ms }, None)?;
        }
        Ok(ReadyCollectionCancellation {
            loaded,
            changed,
            ownership_lifetime: owner.lifetime().into(),
            root: self.root.clone(),
        })
    }
    pub(crate) fn commit_collection_cancellation(
        &mut self,
        ready: ReadyCollectionCancellation,
        owner: &CollectionOwnership,
    ) -> Result<()> {
        self.collection_owner(owner)?;
        require(
            self.root == ready.root && owner.lifetime() == ready.ownership_lifetime,
            "Prepared collection cancellation ownership changed",
        )?;
        // Even idempotent acknowledgement rejects an intervening canonical write;
        // the coordinator never adopts a newer revision or retries implicitly.
        require(
            self.revision()? == ready.loaded.revision,
            "Collection changed before cancellation publication",
        )?;
        self.collection_transport_available()?;
        if ready.changed {
            self.publish_collection(&ready.loaded, None, None)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "collection_cancellation_tests.rs"]
mod tests;
