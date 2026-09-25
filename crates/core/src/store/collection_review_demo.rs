//! Fixed local UI specimens. No external I/O, supplied receipts or executor override.
use super::*;
use crate::collection_transport::{
    Observation, Outcome, Phase, ResolvedCandidates, ResponseHead, StopReason,
};
const AT: i64 = 1_700_000_000_000;
impl Workspace {
    pub fn seed_durable_collection_review(&mut self) -> Result<()> {
        require(self.revision()? == 0, "Use a fresh synthetic workspace")?;
        let owner = self.collection_ownership()?;
        for n in 0..30 {
            let label = match n {
                24 => "recognized-source",
                25 => "blocked",
                26 => "quota",
                27 => "network-failed",
                28 => "interrupted-unknown",
                29 => "queued",
                _ => "cancelled",
            };
            let at = AT + i64::from(n) * 100_000;
            let job = self.queue_collection_protocol(
                CollectionInput {
                    urls: vec![if n < 2 {
                        "https://collection.example/cancelled/shared".into()
                    } else {
                        format!("https://collection.example/{label}/{n}")
                    }],
                    max_hops: 2,
                    max_requests: if n == 26 { 1 } else { 50 },
                    max_seconds: 600,
                },
                &id(),
                at,
                // The acknowledgement pair models current admission. Other
                // specimens intentionally retain historical v3 identities.
                if n < 2 {
                    CollectionProtocol::SyntheticV4
                } else {
                    CollectionProtocol::SyntheticV3
                },
            )?;
            if n < 24 {
                self.cancel_durable_collection(&job.id, 1, at + 1)?;
            } else if n != 29 {
                let execution = self
                    .start_durable_collection(&job.id, 1, &owner, at + 1)?
                    .ok_or_else(|| {
                        Error::Validation("Synthetic collection did not start".into())
                    })?;
                let request = self
                    .advance_durable_collection(&execution, &owner, at + 2)?
                    .ok_or_else(|| Error::Validation("Synthetic robots request missing".into()))?;
                if n == 28 {
                    self.recover_collections(
                        &owner,
                        at + 3,
                        Some(CollectionProtocol::SyntheticV3),
                    )?;
                } else {
                    let observation = if n == 27 {
                        Observation {
                            outcome: Outcome::Stopped {
                                reason: StopReason::Network,
                                head: None,
                            },
                            phase: Phase::Dns,
                            elapsed_milliseconds: 10,
                            observed_wall_ms: at + 3,
                            resolved: None,
                            resolver_uncertainty: None,
                            stop_observed: None,
                            locally_quiescent: true,
                        }
                    } else {
                        complete(at + 3, if n == 25 { 403 } else { 404 }, b"")
                    };
                    self.settle_collection_transport(&request, &observation, &owner)?;
                    if n == 24 {
                        let request = self
                            .advance_durable_collection(&execution, &owner, at + 4)?
                            .ok_or_else(|| {
                                Error::Validation("Synthetic seed request missing".into())
                            })?;
                        self.settle_collection_transport(&request, &complete(at + 5, 200,
                            b"Synthetic retained source. <script>not executable</script>\nNo real collection occurred."), &owner)?;
                    }
                    if self.inspect_durable_collection(&job.id)?.checkpoint.state
                        == CollectionState::Running
                    {
                        require(
                            self.advance_durable_collection(&execution, &owner, at + 6)?
                                .is_none(),
                            "Synthetic fixture left an unexpected request",
                        )?;
                    }
                }
            }
            // Exercise the same replay/original/key validation as public readers.
            self.inspect_collection_run(&job.id)?;
        }
        owner.release()?;
        Ok(())
    }
}
fn complete(at: i64, status: u16, body: &[u8]) -> Observation {
    Observation {
        outcome: Outcome::Complete {
            head: ResponseHead {
                status,
                media_type: Some("text/plain".into()),
                redirect_url: None,
                identity_encoding: true,
            },
            body: body.to_vec(),
        },
        phase: Phase::Body,
        elapsed_milliseconds: 12,
        observed_wall_ms: at,
        resolved: Some(ResolvedCandidates {
            addresses: vec!["1.1.1.1:443".parse().expect("fixed synthetic address")],
            method: "synthetic_fixed_candidates",
            authoritative_complete_set: false,
        }),
        resolver_uncertainty: None,
        stop_observed: None,
        locally_quiescent: true,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection_api::{CollectionAvailability, CollectionRunPageRequest};
    #[test]
    fn fixed_review_seed_is_replayable_bounded_synthetic_and_read_only_in_standalone() {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut w = Workspace::open(temp.path().join("case")).unwrap();
        w.seed_durable_collection_review().unwrap();
        let mut page = w
            .page_collection_runs(
                &CollectionRunPageRequest {
                    page_size: 25,
                    cursor: None,
                },
                None,
            )
            .unwrap();
        assert_eq!(page.scope_count, 30);
        assert_eq!(page.rows.len(), 25);
        let mut rows = page.rows;
        page = w
            .page_collection_runs(
                &CollectionRunPageRequest {
                    page_size: 25,
                    cursor: page.next_cursor,
                },
                None,
            )
            .unwrap();
        rows.extend(page.rows);
        assert_ne!(rows[0].request_key, rows[1].request_key);
        assert_eq!(rows[0].input, rows[1].input);
        for row in &rows {
            let read = w.inspect_collection_run(&row.id).unwrap();
            assert_eq!(read.run.request_key, row.request_key);
            assert_eq!(
                Uuid::parse_str(&row.request_key).unwrap().to_string(),
                row.request_key
            );
        }
        let states = rows.iter().map(|r| r.state).collect::<Vec<_>>();
        for expected in [
            CollectionState::Successful,
            CollectionState::Blocked,
            CollectionState::QuotaExhausted,
            CollectionState::Failed,
            CollectionState::Interrupted,
            CollectionState::Queued,
            CollectionState::Cancelled,
        ] {
            assert!(states.contains(&expected), "{expected:?}: {states:?}");
        }
        for row in rows {
            let read = w.inspect_collection_run(&row.id).unwrap();
            assert_eq!(
                read.availability,
                CollectionAvailability::StandaloneUnavailable
            );
            assert!(
                !read.controls.can_cancel
                    && !read.controls.can_resume
                    && !read.controls.can_retry_settlement
            );
            assert_eq!(
                read.run.mode,
                crate::collection_receipt::AcquisitionMode::Synthetic
            );
        }
        let revision = w.revision().unwrap();
        assert!(w.seed_durable_collection_review().is_err());
        assert_eq!(w.revision().unwrap(), revision);
    }
}
