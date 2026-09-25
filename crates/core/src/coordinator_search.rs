//! Search is synchronous but participates in workspace execution ownership and joined teardown.
use super::*;

impl JobCoordinator {
    pub(super) fn dispatch_search(&self, query: &str) -> Result<Value> {
        self.dispatch_search_with(|workspace, owner| workspace.search_owned(owner, query))
    }
    fn dispatch_search_with(
        &self,
        execute: impl FnOnce(
            &mut Workspace,
            &CollectionOwnership,
        ) -> Result<crate::engines::SearchResults>,
    ) -> Result<Value> {
        let mut workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        if self.shared.stopping.load(Ordering::Acquire) || !self.shared.ownership.held() {
            return Err(Error::Blocked(
                "Workspace execution is stopping, released or quarantined".into(),
            ));
        }
        let result = execute(&mut workspace, &self.shared.ownership);
        let quarantined = !self.shared.ownership.held();
        drop(workspace);
        if quarantined {
            quarantine_execution(&self.shared);
        }
        result.and_then(|result| Ok(serde_json::to_value(result)?))
    }
}

#[cfg(test)]
#[path = "coordinator_search_tests.rs"]
mod tests;
