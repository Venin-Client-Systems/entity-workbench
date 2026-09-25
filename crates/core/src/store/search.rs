//! Synchronous Search shares the one workspace execution owner; it never recovers an intent.
use super::*;
use crate::engines::{
    search_lifecycle::{Completion, Disposition},
    SearchResults,
};

struct SearchPermit<'a> {
    owner: &'a CollectionOwnership,
    completed: bool,
}
impl Drop for SearchPermit<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.owner.quarantine();
        }
    }
}
impl Workspace {
    pub(crate) fn search_recovery_required(&self) -> bool {
        crate::engines::search_lifecycle::recovery_required(&self.root.join("indexes/lucene"))
    }
    pub(super) fn search_standalone(&mut self, query: &str) -> Result<SearchResults> {
        let owner = self.collection_ownership()?;
        // With exclusive ownership, retained Running claims cannot be a current coordinator.
        // Refuse rather than perform canonical recovery writes as a side effect of Search.
        let orphaned: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE (kind='processing_job' AND (json_extract(body,'$.state')='running' OR (json_extract(body,'$.failure')='interrupted' AND json_extract(body,'$.lease') IS NULL))) OR (kind='collection_run' AND json_extract(body,'$.checkpoint.state')='running'))",
            [], |row| row.get(0))?;
        if orphaned {
            owner.quarantine();
        }
        self.search_owned(&owner, query)
    }
    pub(crate) fn search_owned(
        &mut self,
        owner: &CollectionOwnership,
        query: &str,
    ) -> Result<SearchResults> {
        self.search_owned_with(owner, query, |runtime, cache, revision, evidence, query| {
            runtime.search_completed(cache, revision, evidence, query)
        })
    }
    pub(crate) fn search_owned_with(
        &mut self,
        owner: &CollectionOwnership,
        query: &str,
        execute: impl FnOnce(
            &crate::engines::Runtime,
            &Path,
            u64,
            &[Evidence],
            &str,
        ) -> Completion<SearchResults>,
    ) -> Result<SearchResults> {
        self.collection_owner(owner).map_err(|_| Error::Blocked("Search execution ownership is released, quarantined or belongs to another workspace".into()))?;
        if self.processing_execution_suspended()? || self.search_recovery_required() {
            owner.quarantine();
            return Err(Error::Blocked(
                "Search requires verified workspace execution recovery".into(),
            ));
        }
        if let Err(error) = self.collection_transport_available() {
            owner.quarantine();
            return Err(match error {
                Error::Validation(message) => Error::Blocked(message),
                error => error,
            });
        }
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            Error::Blocked("Packaged local search runtime is not available in this build".into())
        })?;
        let revision = self.revision()?;
        let evidence = all_evidence(&self.conn)?;
        let mut permit = SearchPermit {
            owner,
            completed: false,
        };
        let completion = execute(
            runtime,
            &self.root.join("indexes/lucene"),
            revision,
            &evidence,
            query,
        );
        if completion.disposition == Disposition::Released {
            permit.completed = true;
        }
        // This drop quarantines before the caller can release the workspace mutex.
        drop(permit);
        completion.result
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
