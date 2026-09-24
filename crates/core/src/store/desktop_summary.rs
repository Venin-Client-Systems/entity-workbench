//! Derive the smaller response from one captured presentation and the existing calculator.
use super::*;
use crate::{desktop_summary::*, transaction_page::TransactionReviewCounts};

impl Workspace {
    pub fn desktop_summary(&self) -> Result<DesktopSummaryResponse> {
        // presentation() captures every retained field, row and revision in one
        // read snapshot. Do not query fresh counts or a revision after it returns.
        let view = self.presentation()?;
        let calculated = analytics::analyse(&view.transactions)?;
        let mut review_counts = TransactionReviewCounts::default();
        for row in &view.transactions {
            match row.review {
                ReviewState::Accepted => review_counts.accepted += 1,
                ReviewState::Pending => review_counts.pending += 1,
                ReviewState::Rejected => review_counts.rejected += 1,
                ReviewState::Deferred => review_counts.deferred += 1,
            }
        }
        let analysis = LedgerSummary {
            transaction_count: view.transactions.len() as u64,
            review_counts,
            duplicate_candidate_row_count: calculated.duplicate_candidates as u64,
            balance_check_count: calculated.balance_checks.len() as u64,
            balance_discrepancy_count: calculated
                .balance_checks
                .iter()
                .filter(|check| !check.reconciled)
                .count() as u64,
            totals: calculated
                .totals
                .into_iter()
                .map(|total| ReviewedCurrencySummary {
                    currency: total.currency,
                    credits: total.credits,
                    debits: total.debits,
                    net: total.net,
                    included_count: total.transaction_ids.len() as u64,
                    excluded_transfer_count: total.excluded_transfer_ids.len() as u64,
                })
                .collect(),
        };
        let WorkspaceView {
            schema_version,
            revision,
            entities,
            evidence,
            observations,
            assertions,
            transactions: _,
            addresses,
            locations,
            leads,
            jobs,
            findings,
            hypotheses,
            decisions,
            merges,
            identity_decisions,
            reports,
            statement_profiles,
            statement_imports,
        } = view;
        Ok(DesktopSummaryResponse {
            schema_version: 1,
            workspace: DesktopWorkspace {
                schema_version,
                revision,
                review_decision_count: decisions.len() as u64,
                entities,
                evidence,
                observations,
                assertions,
                addresses,
                locations,
                leads,
                jobs,
                findings,
                hypotheses,
                merges,
                identity_decisions,
                reports,
                statement_profiles,
                statement_imports,
            },
            analysis,
        })
    }
}

#[cfg(test)]
#[path = "desktop_summary_tests.rs"]
mod tests;
