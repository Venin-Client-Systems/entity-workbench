//! Fixed development proof only. The ignored native case has a closed two-call guard.
//! Normal production admission remains disabled; no caller-supplied URL or executor.
use super::*;
use crate::{
    collection_api::{CollectionRunInspection, NATIVE_COLLECTION_ENABLED},
    collection_jobs::{
        CollectionInput, CollectionProtocol, CollectionState, RequestProgress, RequestTicket,
    },
    collection_settlement::{ReceiptOutcome, TransportReceipt},
    collection_transport::{ExecutionWindow, Observation, Outcome, Phase, StopReason},
};
use serde_json::json;
use std::time::Instant;

const POLICY: &str = "fixed-owned-synthetic-https-two-attempts-v1";
const PREFIX: &str = "EW_NATIVE_HTTPS_EVENT=";
const SEED: &str = "https://raw.githubusercontent.com/Venin-Client-Systems/entity-workbench/df4dbff063d274effb9fa6385095938751bb300e/fixtures/brief.txt";
const ROBOTS: &str = "https://raw.githubusercontent.com/robots.txt";
const FIXTURE_SHA: &str = "1b728541f9939c78bd3432482c1b5894ff70ffc017ebbe07a2f4b0535ba99371";
const FIXTURE: &[u8] = include_bytes!("../../../fixtures/brief.txt");
fn input() -> CollectionInput {
    CollectionInput {
        urls: vec![SEED.into()],
        max_hops: 0,
        max_requests: 2,
        max_seconds: 20,
    }
}
fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

struct Guard {
    job_id: String,
    lease: Option<String>,
    admitted: u32,
    refused: bool,
    deadline: Instant,
}
impl Guard {
    fn admit(&mut self, ticket: &RequestTicket, scope: &CollectionInput) -> Result<()> {
        let valid = !self.refused
            && Instant::now() < self.deadline
            && scope == &input()
            && self.admitted < 2
            && ticket.sequence == self.admitted
            && ticket.url == [ROBOTS, SEED][self.admitted.min(1) as usize]
            && ticket.run.job_id == self.job_id
            && ticket.run.generation == 1
            && uuid::Uuid::parse_str(&ticket.run.lease)
                .is_ok_and(|id| id.to_string() == ticket.run.lease)
            && self
                .lease
                .as_ref()
                .is_none_or(|lease| lease == &ticket.run.lease);
        if !valid {
            self.refused = true;
            return Err(Error::Blocked(
                "Fixed HTTPS campaign guard refused execution".into(),
            ));
        }
        self.lease.get_or_insert_with(|| ticket.run.lease.clone());
        self.admitted += 1;
        Ok(())
    }
}
#[derive(Clone)]
struct Context {
    source: String,
    nonce: String,
    emit: bool,
    policy: &'static str,
}
impl Context {
    fn event(&self, kind: &str, details: Value) {
        if self.emit {
            println!(
                "{PREFIX}{}",
                json!({"policy":self.policy,"source":self.source,
                "nonce":self.nonce,"kind":kind,"details":details})
            );
        }
    }
}
fn refused_observation() -> Observation {
    Observation {
        outcome: Outcome::Stopped {
            reason: StopReason::Policy,
            head: None,
        },
        phase: Phase::BeforeRequest,
        elapsed_milliseconds: 0,
        observed_wall_ms: now(),
        resolved: None,
        resolver_uncertainty: None,
        stop_observed: None,
        locally_quiescent: true,
    }
}

/// The live caller is fixed below. Ordinary tests inject explicitly synthetic results.
fn campaign(
    root: &Path,
    context: &Context,
    protocol: CollectionProtocol,
    executor: Arc<collection::CollectionExecutor>,
) -> Result<Value> {
    campaign_controlled(
        root,
        context,
        protocol,
        executor,
        Expected::Successful,
        |_, _| Ok(()),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Expected {
    Successful,
    Cancelled,
}
fn campaign_controlled(
    root: &Path,
    context: &Context,
    protocol: CollectionProtocol,
    executor: Arc<collection::CollectionExecutor>,
    expected: Expected,
    mut controller: impl FnMut(&JobCoordinator, &str) -> Result<()>,
) -> Result<Value> {
    crate::require(
        !NATIVE_COLLECTION_ENABLED,
        "Production collection must remain disabled",
    )?;
    crate::require(
        crate::store::hash(FIXTURE) == FIXTURE_SHA && FIXTURE.len() == 863,
        "Frozen synthetic fixture changed",
    )?;
    crate::require(
        !root.join("workspace").exists() && !root.join("restored").exists(),
        "Use a fresh proof directory",
    )?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(20);
    let mut workspace = Workspace::open(root.join("workspace"))?;
    let disclosure =
        crate::collection_api::preview_policy(input(), CollectionProtocol::NativeV3.policy())?;
    let confirmed = crate::collection_api::confirmed_policy(
        input(),
        &disclosure.preview_sha256,
        CollectionProtocol::NativeV3.policy(),
    )?;
    // Deliberately private canonical admission. Normal public queue is still refused.
    let queued = workspace.queue_collection_protocol(confirmed, &context.nonce, now(), protocol)?;
    context.event(
        "queued",
        json!({"preview":disclosure,"job_id":queued.id,
        "request_key":context.nonce,"record_version":3,"synthetic":queued.synthetic,
        "production_native_enabled":NATIVE_COLLECTION_ENABLED}),
    );
    let guard = Arc::new(Mutex::new(Guard {
        job_id: queued.id.clone(),
        lease: None,
        admitted: 0,
        refused: false,
        deadline,
    }));
    let observations = Arc::new(Mutex::new(Vec::<TransportReceipt>::new()));
    let run_guard = guard.clone();
    let retained = observations.clone();
    let events = context.clone();
    let coordinator = JobCoordinator::with_execution(
        workspace,
        1,
        Arc::new(|_, _, _, _, _| {
            Err(Error::Blocked(
                "No document execution in HTTPS proof".into(),
            ))
        }),
        Some((
            Arc::new(
                move |ticket, scope, window: ExecutionWindow, pacing, token| {
                    // This guard runs before calling fetch, before native resolver or sockets.
                    if run_guard
                        .lock()
                        .map_or(true, |mut g| g.admit(ticket, scope).is_err())
                    {
                        events.event("guard_refused", json!({"sequence":ticket.sequence}));
                        return refused_observation();
                    }
                    events.event(
                        "launch",
                        json!({"sequence":ticket.sequence,"url":ticket.url,
                "job_id":ticket.run.job_id,"generation":ticket.run.generation}),
                    );
                    let observation = executor(ticket, scope, window, pacing, token);
                    let receipt = TransportReceipt::from_observation(&observation);
                    events.event(
                        "observation",
                        json!({"sequence":ticket.sequence,"receipt":receipt}),
                    );
                    retained
                        .lock()
                        .expect("Owned proof observations")
                        .push(receipt);
                    observation
                },
            ),
            protocol,
        )),
    )?;
    let mut control_failed = false;
    loop {
        if controller(&coordinator, &queued.id).is_err() {
            control_failed = true;
            context.event(
                "controller_failed",
                json!({"reason":"cancellation_control_failed"}),
            );
            break;
        }
        let job = coordinator.inspect_collection_job(&queued.id)?;
        let status = coordinator.collection_status()?;
        if !matches!(
            job.checkpoint.state,
            CollectionState::Queued | CollectionState::Running
        ) || matches!(
            status.phase,
            collection::LanePhase::SettlementPending
                | collection::LanePhase::Faulted
                | collection::LanePhase::Unpublished
                | collection::LanePhase::RecoveryRequired
                | collection::LanePhase::Stopped
        ) || Instant::now() >= deadline
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Shutdown cancels/joins the actual existing lanes. Unknown completion remains distinct.
    let shutdown = coordinator.shutdown();
    let shutdown_ok = shutdown.is_ok();
    let ownership_released = !coordinator.shared.ownership.publication_held();
    let workspace = coordinator
        .shared
        .workspace
        .lock()
        .map_err(|_| Error::Blocked("Proof workspace unavailable".into()))?;
    let inspection = workspace.inspect_collection_run(&queued.id)?;
    let canonical = workspace.inspect_durable_collection(&queued.id)?;
    let observations_unchanged = workspace.view()?.observations.is_empty();
    drop(workspace);
    drop(coordinator);
    let mut reopened_verified = false;
    let mut backup_restored_verified = false;
    let mut terminal_resume_refused = expected == Expected::Successful;
    if shutdown_ok && ownership_released {
        let mut reopened = Workspace::open(root.join("workspace"))?;
        reopened_verified = reopened.inspect_durable_collection(&queued.id)? == canonical
            && reopened.inspect_collection_run(&queued.id)?.requests == inspection.requests;
        if expected == Expected::Cancelled {
            // A normal offline coordinator owns the reopen check. It has no collection lane.
            let check = JobCoordinator::start(Workspace::open(root.join("workspace"))?, 1)?;
            {
                let mut workspace = check
                    .shared
                    .workspace
                    .lock()
                    .map_err(|_| Error::Blocked("Reopen check unavailable".into()))?;
                let revision = workspace.revision()?;
                terminal_resume_refused = matches!(
                    workspace.start_durable_collection(
                        &queued.id,
                        canonical.checkpoint.generation,
                        &check.shared.ownership,
                        now()
                    ),
                    Err(Error::Conflict(_))
                ) && workspace.revision()? == revision
                    && workspace.inspect_durable_collection(&queued.id)? == canonical;
            }
            check.shutdown()?;
            drop(check);
        }
        let backup = reopened.backup()?;
        let restored = Workspace::restore(&backup, &root.join("restored"))?;
        backup_restored_verified = restored.inspect_durable_collection(&queued.id)? == canonical
            && restored.inspect_collection_run(&queued.id)?.requests == inspection.requests
            && restored.view()?.observations.is_empty();
    }
    let guard = guard
        .lock()
        .map_err(|_| Error::Blocked("Proof guard unavailable".into()))?;
    let receipts = observations
        .lock()
        .map_err(|_| Error::Blocked("Proof receipts unavailable".into()))?;
    let receipt_settled_exactly = receipts.len() == inspection.requests.len()
        && receipts.iter().zip(&inspection.requests).all(|(receipt, request)| {
            matches!(&request.progress, RequestProgress::Observed { receipt: stored } if receipt == stored)
        });
    let acquisition_valid = match expected {
        Expected::Successful => successful_acquisition(&inspection, protocol.synthetic()),
        Expected::Cancelled => {
            cancellation::cancelled_acquisition(&inspection, protocol.synthetic())
        }
    };
    let passed = !control_failed
        && terminal_resume_refused
        && shutdown_ok
        && ownership_released
        && reopened_verified
        && backup_restored_verified
        && observations_unchanged
        && receipt_settled_exactly
        && acquisition_valid
        && !guard.refused
        && guard.admitted == 2;
    let mut report = json!({"passed":passed,"synthetic":protocol.synthetic(),"inspection":inspection,
        "guard":{"admitted":guard.admitted,"refused":guard.refused},
        "shutdown_ok":shutdown_ok,"ownership_released":ownership_released,
        "reopened_verified":reopened_verified,"backup_restored_verified":backup_restored_verified,
        "observations_unchanged":observations_unchanged,"receipt_settled_exactly":receipt_settled_exactly,
        "acquisition_valid":acquisition_valid,"elapsed_milliseconds":started.elapsed().as_millis(),
        "fixture_sha256":FIXTURE_SHA,"fixture_bytes":FIXTURE.len(),
        "canonical_record_sha256":crate::store::hash(&serde_json::to_vec(&canonical)?),
        "production_native_enabled":NATIVE_COLLECTION_ENABLED});
    if expected == Expected::Cancelled {
        report["terminal_resume_refused"] = json!(terminal_resume_refused);
        report["control_failed"] = json!(control_failed);
    }
    Ok(report)
}
fn successful_acquisition(inspection: &CollectionRunInspection, synthetic: bool) -> bool {
    let expected_mode = if synthetic {
        crate::collection_receipt::AcquisitionMode::Synthetic
    } else {
        crate::collection_receipt::AcquisitionMode::Live
    };
    if inspection.run.mode != expected_mode
        || inspection.run.record_version != 3
        || inspection.run.input != input()
        || inspection.run.requests_used != 2
        || inspection.run.pages_retained != 1
        || inspection.run.state != CollectionState::Successful
        || inspection.requests.len() != 2
        || inspection.run.cancellation_requested
        || inspection.native_execution_enabled
    {
        return false;
    }
    for (index, request) in inspection.requests.iter().enumerate() {
        let RequestProgress::Observed { receipt } = &request.progress else {
            return false;
        };
        let ReceiptOutcome::Complete {
            head,
            sha256,
            bytes,
        } = &receipt.outcome
        else {
            return false;
        };
        let Some(original) = &request.original else {
            return false;
        };
        let Some(candidates) = &receipt.resolved else {
            return false;
        };
        let purpose = if index == 0 {
            crate::collection_jobs::Purpose::Robots
        } else {
            crate::collection_jobs::Purpose::Seed
        };
        if request.sequence != index as u32
            || request.generation != 1
            || request.entry.url != [ROBOTS, SEED][index]
            || request.entry.purpose != purpose
            || request.entry.hop != 0
            || request.entry.redirects != 0
            || request.entry.parent.is_some()
            || !receipt.locally_quiescent
            || receipt.stop_observed.is_some()
            || receipt.resolver_uncertainty.is_some()
            || !head.identity_encoding
            || head.redirect_url.is_some()
            || candidates.authoritative_complete_set
            || candidates.addresses.is_empty()
            || candidates.addresses.len() > 64
            || !candidates
                .addresses
                .iter()
                .all(|a| crate::policy::public_ip(a.ip()) && a.port() == 443)
            || candidates.method
                != if synthetic {
                    "synthetic_fixed_candidates"
                } else {
                    "macos_dns_service_observed_batch"
                }
            || original.sha256 != *sha256
            || original.evidence_id != *sha256
            || original.bytes != *bytes
            || (index == 0 && ![200, 404].contains(&head.status))
            || (index == 1 && (head.status != 200 || sha256 != FIXTURE_SHA || *bytes != 863))
        {
            return false;
        }
    }
    true
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "fixed native HTTPS proof; requires separately reviewed signed source and explicit opt-in"]
fn native_durable_https_campaign() {
    assert_eq!(
        std::env::var("EW_NATIVE_HTTPS_PROOF").as_deref(),
        Ok(POLICY)
    );
    let source = option_env!("EW_NATIVE_HTTPS_BUILD_SOURCE").unwrap_or("");
    assert!(
        source.len() == 40
            && source
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_eq!(
        std::env::var("EW_NATIVE_HTTPS_SOURCE").as_deref(),
        Ok(source)
    );
    let nonce = std::env::var("EW_NATIVE_HTTPS_NONCE").expect("Campaign nonce");
    assert_eq!(
        uuid::Uuid::parse_str(&nonce)
            .expect("Canonical nonce")
            .to_string(),
        nonce
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
        .join("artifacts/native-durable-https")
        .join(&nonce);
    assert!(root.is_dir() && !root.is_symlink());
    assert_eq!(
        root.canonicalize().unwrap(),
        root,
        "Proof path contains an alias"
    );
    let context = Context {
        source: source.into(),
        nonce,
        emit: true,
        policy: POLICY,
    };
    context.event(
        "start",
        json!({"fixture_sha256":FIXTURE_SHA,"fixture_bytes":863,
        "max_requests":2,"max_hops":0,"max_seconds":20,"synthetic":false}),
    );
    let result = campaign(
        &root,
        &context,
        CollectionProtocol::NativeV3,
        Arc::new(crate::collection_transport::fetch),
    );
    match result {
        Ok(report) => {
            context.event("final", report.clone());
            assert_eq!(
                report["passed"], true,
                "Native HTTPS proof did not pass; retained failure is authoritative"
            );
        }
        Err(_) => {
            context.event(
                "failed",
                json!({"reason":"canonical_or_owned_operation_failed"}),
            );
            panic!("Native HTTPS proof failed; inspect retained local evidence");
        }
    }
}

#[test]
fn fixed_https_guard_refuses_drift_reordering_extra_attempts_and_expiry_before_executor() {
    use crate::collection_jobs::CollectionTicket;
    let ticket = |sequence, url: &str| RequestTicket {
        run: CollectionTicket {
            job_id: "fixed-run".into(),
            generation: 1,
            lease: "00000000-0000-4000-8000-000000000002".into(),
        },
        sequence,
        url: url.into(),
    };
    let fresh = || Guard {
        job_id: "fixed-run".into(),
        lease: None,
        admitted: 0,
        refused: false,
        deadline: Instant::now() + Duration::from_secs(1),
    };
    let mut valid = fresh();
    valid.admit(&ticket(0, ROBOTS), &input()).unwrap();
    valid.admit(&ticket(1, SEED), &input()).unwrap();
    assert!(valid.admit(&ticket(2, SEED), &input()).is_err());
    assert_eq!(valid.admitted, 2);
    for changed in [
        ticket(1, SEED),
        ticket(0, SEED),
        ticket(0, "https://other.invalid/robots.txt"),
    ] {
        let mut guard = fresh();
        assert!(guard.admit(&changed, &input()).is_err());
        assert_eq!(guard.admitted, 0);
        assert!(guard.admit(&ticket(0, ROBOTS), &input()).is_err());
    }
    let mut guard = fresh();
    let mut scope = input();
    scope.max_requests = 3;
    assert!(guard.admit(&ticket(0, ROBOTS), &scope).is_err());
    let mut guard = fresh();
    guard.deadline = Instant::now();
    assert!(guard.admit(&ticket(0, ROBOTS), &input()).is_err());
    let mut guard = fresh();
    guard.admit(&ticket(0, ROBOTS), &input()).unwrap();
    let mut changed = ticket(1, SEED);
    changed.run.lease = uuid::Uuid::new_v4().to_string();
    assert!(guard.admit(&changed, &input()).is_err());
}
fn synthetic(sequence: u32, body: &[u8], robots_status: u16) -> Observation {
    use crate::collection_transport::{ResolvedCandidates, ResponseHead};
    Observation {
        outcome: Outcome::Complete {
            head: ResponseHead {
                status: if sequence == 0 { robots_status } else { 200 },
                media_type: Some("text/plain".into()),
                redirect_url: None,
                identity_encoding: true,
            },
            body: if sequence == 0 {
                Vec::new()
            } else {
                body.to_vec()
            },
        },
        phase: Phase::Body,
        elapsed_milliseconds: 0,
        observed_wall_ms: now(),
        resolved: Some(ResolvedCandidates {
            addresses: vec!["1.1.1.1:443".parse().unwrap()],
            method: "synthetic_fixed_candidates",
            authoritative_complete_set: false,
        }),
        resolver_uncertainty: None,
        stop_observed: None,
        locally_quiescent: true,
    }
}
#[test]
fn fixed_https_orchestration_replays_and_restores_real_synthetic_canonical_receipts() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let context = Context {
        source: "offline".into(),
        nonce: uuid::Uuid::new_v4().to_string(),
        emit: false,
        policy: POLICY,
    };
    let report = campaign(
        temp.path(),
        &context,
        CollectionProtocol::SyntheticV3,
        Arc::new(|ticket, _, _, _, _| synthetic(ticket.sequence, FIXTURE, 404)),
    )
    .unwrap();
    assert_eq!(report["passed"], true);
    assert_eq!(report["synthetic"], true);
    assert_eq!(report["inspection"]["run"]["request_key"], context.nonce);
    assert_eq!(report["production_native_enabled"], false);
}
#[test]
fn fixed_https_orchestration_preserves_robots_refusal_and_changed_fixture_as_failures() {
    for (robots_status, body, count) in [(403, FIXTURE, 1), (404, b"changed".as_slice(), 2)] {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let context = Context {
            source: "offline".into(),
            nonce: uuid::Uuid::new_v4().to_string(),
            emit: false,
            policy: POLICY,
        };
        let report = campaign(
            temp.path(),
            &context,
            CollectionProtocol::SyntheticV3,
            Arc::new(move |ticket, _, _, _, _| synthetic(ticket.sequence, body, robots_status)),
        )
        .unwrap();
        assert_eq!(report["passed"], false);
        assert_eq!(report["guard"]["admitted"], count);
        assert_eq!(report["reopened_verified"], true);
        assert_eq!(report["backup_restored_verified"], true);
        assert_eq!(report["receipt_settled_exactly"], true);
        assert_eq!(report["observations_unchanged"], true);
    }
}

#[test]
fn fixed_https_orchestration_retains_unknown_completion_without_recovery_claims() {
    use crate::collection_transport::{CallerContextState, ResolverUncertainty};
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let context = Context {
        source: "offline".into(),
        nonce: uuid::Uuid::new_v4().to_string(),
        emit: false,
        policy: POLICY,
    };
    let report = campaign(
        temp.path(),
        &context,
        CollectionProtocol::SyntheticV3,
        Arc::new(|_, _, _, _, _| Observation {
            outcome: Outcome::Stopped {
                reason: StopReason::QuiescenceUnverified,
                head: None,
            },
            phase: Phase::Dns,
            elapsed_milliseconds: 1,
            observed_wall_ms: now(),
            resolved: None,
            resolver_uncertainty: Some(ResolverUncertainty {
                method: "windows_overlapped_dns",
                caller_context: CallerContextState::ReleasedAfterCompletion,
            }),
            stop_observed: None,
            locally_quiescent: false,
        }),
    )
    .unwrap();
    assert_eq!(report["passed"], false);
    assert_eq!(report["guard"]["admitted"], 1);
    assert_eq!(report["shutdown_ok"], false);
    assert_eq!(report["ownership_released"], false);
    assert_eq!(report["reopened_verified"], false);
    assert_eq!(report["backup_restored_verified"], false);
    assert_eq!(report["receipt_settled_exactly"], true);
    assert_eq!(report["inspection"]["run"]["state"], "recovery_required");
    assert_eq!(report["inspection"]["requests"][0]["original"], Value::Null);
}

#[path = "coordinator_native_https_cancellation.rs"]
mod cancellation;
