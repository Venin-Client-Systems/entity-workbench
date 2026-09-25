//! One owned native response cancellation point, not a complete activation gate.
use super::*;
#[cfg(target_os = "macos")]
use crate::collection_transport::cancellation_probe::fetch_with_gate;
use crate::collection_transport::{cancellation_probe::ResponseGate, ResponseHead};
use std::sync::mpsc::TryRecvError;

const CANCEL_POLICY: &str = "fixed-owned-response-cancellation-v1";
type GatedExecutor = dyn Fn(
        &RequestTicket,
        &CollectionInput,
        ExecutionWindow,
        Instant,
        &CancellationToken,
        Option<&ResponseGate>,
    ) -> Observation
    + Send
    + Sync;

fn run_cancel(
    root: &Path,
    context: &Context,
    protocol: CollectionProtocol,
    executor: Arc<GatedExecutor>,
) -> Result<Value> {
    let (gate, reached) = ResponseGate::new();
    let gate = Arc::new(gate);
    let worker_gate = Arc::clone(&gate);
    let mut acknowledged = None;
    let mut headers = None;
    let mut report = campaign_controlled(
        root,
        context,
        protocol,
        Arc::new(move |ticket, input, window, pacing, token| {
            executor(
                ticket,
                input,
                window,
                pacing,
                token,
                (ticket.sequence == 1).then_some(worker_gate.as_ref()),
            )
        }),
        Expected::Cancelled,
        |coordinator, id| {
            if acknowledged.is_some() {
                return Ok(());
            }
            match reached.try_recv() {
                Ok(head) => {
                    context.event("response_headers", json!({"sequence":1,"head":head}));
                    headers = Some(head);
                    // Intent is emitted before the call; acknowledgement and observation may race.
                    context.event(
                        "cancel_intent",
                        json!({"job_id":id,"generation":1,"sequence":1}),
                    );
                    let job = coordinator.cancel_collection(id, 1)?;
                    let ack = json!({"job_id":job.id,"generation":job.checkpoint.generation,
                        "cancellation_requested":job.checkpoint.cancellation_requested,
                        "requests_used":job.checkpoint.requests_used(),"state":job.checkpoint.state});
                    context.event("cancel_acknowledged", ack.clone());
                    acknowledged = Some(ack);
                }
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => {
                    return Err(Error::Blocked("Response gate unavailable".into()))
                }
            }
            Ok(())
        },
    )?;
    let state = gate.state();
    let ack_valid = acknowledged.as_ref().is_some_and(|ack| {
        ack["cancellation_requested"] == true
            && ack["generation"] == 1
            && ack["requests_used"] == 2
            && ack["job_id"] == report["inspection"]["run"]["id"]
    });
    let headers_valid = headers.as_ref().is_some_and(|head| {
        head.status == 200 && head.identity_encoding && head.redirect_url.is_none()
    });
    let passed = report["passed"] == true
        && ack_valid
        && headers_valid
        && state.reached_headers
        && state.cancellation_observed
        && !state.notification_failed
        && !state.handshake_expired;
    report["passed"] = json!(passed);
    report["gate"] = serde_json::to_value(state)?;
    report["cancel_acknowledgement"] = json!(acknowledged);
    report["response_head"] = json!(headers);
    report["proof_boundary"] = json!("owned_response_before_body_consumption");
    Ok(report)
}

pub(super) fn cancelled_acquisition(inspection: &CollectionRunInspection, synthetic: bool) -> bool {
    let run = &inspection.run;
    let expected_mode = if synthetic {
        crate::collection_receipt::AcquisitionMode::Synthetic
    } else {
        crate::collection_receipt::AcquisitionMode::Live
    };
    if run.mode != expected_mode
        || run.record_version != 3
        || run.input != input()
        || run.requests_used != 2
        || run.pages_retained != 0
        || run.state != CollectionState::Cancelled
        || !run.cancellation_requested
        || inspection.requests.len() != 2
        || inspection.native_execution_enabled
    {
        return false;
    }
    for (index, request) in inspection.requests.iter().enumerate() {
        let RequestProgress::Observed { receipt } = &request.progress else {
            return false;
        };
        let Some(candidates) = &receipt.resolved else {
            return false;
        };
        if request.sequence != index as u32
            || request.generation != 1
            || request.entry.url != [ROBOTS, SEED][index]
            || request.entry.hop != 0
            || request.entry.redirects != 0
            || request.entry.parent.is_some()
            || request.entry.purpose
                != if index == 0 {
                    crate::collection_jobs::Purpose::Robots
                } else {
                    crate::collection_jobs::Purpose::Seed
                }
            || receipt.phase != Phase::Body
            || receipt.http_delivery != crate::collection_settlement::HttpDelivery::MayHaveBeenSent
            || !receipt.locally_quiescent
            || receipt.resolver_uncertainty.is_some()
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
        {
            return false;
        }
        if index == 0 {
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
            if ![200, 404].contains(&head.status)
                || !head.identity_encoding
                || head.redirect_url.is_some()
                || receipt.stop_observed.is_some()
                || original.sha256 != *sha256
                || original.evidence_id != *sha256
                || original.bytes != *bytes
            {
                return false;
            }
        } else {
            let ReceiptOutcome::Stopped {
                reason: StopReason::Cancelled,
                head: Some(head),
            } = &receipt.outcome
            else {
                return false;
            };
            if head.status != 200
                || !head.identity_encoding
                || head.redirect_url.is_some()
                || receipt.stop_observed != Some(StopReason::Cancelled)
                || request.original.is_some()
            {
                return false;
            }
        }
    }
    true
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "fixed native response cancellation; separately reviewed signed source and opt-in required"]
fn native_durable_https_cancellation_campaign() {
    assert_eq!(
        std::env::var("EW_NATIVE_HTTPS_PROOF").as_deref(),
        Ok(CANCEL_POLICY)
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
        .join("artifacts/native-collection-cancellation")
        .join(&nonce);
    assert!(root.is_dir() && !root.is_symlink());
    assert_eq!(root.canonicalize().unwrap(), root);
    let context = Context {
        source: source.into(),
        nonce,
        emit: true,
        policy: CANCEL_POLICY,
    };
    context.event("start",json!({"fixture_sha256":FIXTURE_SHA,"fixture_bytes":863,"max_requests":2,
        "max_hops":0,"max_seconds":20,"synthetic":false,"proof_boundary":"owned_response_before_body_consumption","handshake_seconds":5}));
    let result = run_cancel(
        &root,
        &context,
        CollectionProtocol::NativeV3,
        Arc::new(|ticket, input, window, pacing, token, gate| {
            if let Some(gate) = gate {
                fetch_with_gate(ticket, input, window, pacing, token, gate)
            } else {
                crate::collection_transport::fetch(ticket, input, window, pacing, token)
            }
        }),
    );
    match result {
        Ok(report) => {
            context.event("final", report.clone());
            assert_eq!(
                report["passed"], true,
                "Native cancellation proof failed; retained evidence is authoritative"
            );
        }
        Err(_) => {
            context.event(
                "failed",
                json!({"reason":"canonical_or_owned_operation_failed"}),
            );
            panic!("Native cancellation proof failed; inspect local evidence");
        }
    }
}

fn synthetic_executor(robots_status: u16) -> Arc<GatedExecutor> {
    Arc::new(move |ticket, _, _, _, token, gate| {
        let mut observation = synthetic(ticket.sequence, FIXTURE, robots_status);
        if let Some(gate) = gate {
            let head = ResponseHead {
                status: 200,
                identity_encoding: true,
                media_type: Some("text/plain".into()),
                redirect_url: None,
            };
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = runtime.block_on(gate.pause(&head, token, Duration::from_secs(1)));
            drop(runtime);
            // Explicit synthetic transport fixture; the separate TLS test checks real token consumption.
            let reason = result.err().unwrap_or(StopReason::Cancelled);
            observation.outcome = Outcome::Stopped {
                reason,
                head: Some(head),
            };
            observation.stop_observed = token.is_cancelled().then_some(StopReason::Cancelled);
            observation.observed_wall_ms = now();
        }
        observation
    })
}
#[test]
fn fixed_cancellation_canonical_controller_preserves_charges_and_reopen_without_seed_original() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let context = Context {
        source: "offline".into(),
        nonce: uuid::Uuid::new_v4().to_string(),
        emit: false,
        policy: CANCEL_POLICY,
    };
    let report = run_cancel(
        temp.path(),
        &context,
        CollectionProtocol::SyntheticV3,
        synthetic_executor(404),
    )
    .unwrap();
    assert_eq!(report["passed"], true);
    assert_eq!(report["terminal_resume_refused"], true);
    assert_eq!(report["inspection"]["run"]["state"], "cancelled");
    assert_eq!(report["inspection"]["run"]["pages_retained"], 0);
    assert_eq!(report["inspection"]["requests"][1]["original"], Value::Null);
}
#[test]
fn fixed_cancellation_never_passes_when_robots_blocks_before_handshake() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let context = Context {
        source: "offline".into(),
        nonce: uuid::Uuid::new_v4().to_string(),
        emit: false,
        policy: CANCEL_POLICY,
    };
    let report = run_cancel(
        temp.path(),
        &context,
        CollectionProtocol::SyntheticV3,
        synthetic_executor(403),
    )
    .unwrap();
    assert_eq!(report["passed"], false);
    assert_eq!(report["guard"]["admitted"], 1);
    assert_eq!(report["cancel_acknowledgement"], Value::Null);
    assert_eq!(report["gate"]["reached_headers"], false);
}
