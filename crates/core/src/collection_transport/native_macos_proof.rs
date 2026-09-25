//! Explicitly opted-in OS evidence. Ordinary tests never resolve a public name.
use super::*;
use crate::collection_jobs::CollectionTicket;
use crate::collection_transport::{fetch, Outcome};
use serde_json::{json, Value};
use std::cell::RefCell;

const OPT_IN: &str = "fixed-three-subscriptions-no-http-v2";
const PREFIX: &str = "EW_NATIVE_DNS_CASE=";

#[derive(Default, serde::Serialize)]
struct Probe {
    attempted: u32,
    created: u32,
    deallocated: u32,
    creation_status: Option<i32>,
    callbacks: Vec<(Option<u32>, i32)>,
    #[serde(skip)]
    expire_after_creation: bool,
}
thread_local! {
    // No public or production timing override, global callback thread or shared
    // callback context. DNSServiceProcessResult invokes this on its owning thread.
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
}
fn update(f: impl FnOnce(&mut Probe)) {
    PROBE.with_borrow_mut(|value| {
        if let Some(probe) = value {
            f(probe);
        }
    });
}
pub(super) fn attempted() {
    update(|probe| probe.attempted += 1);
}
pub(super) fn creation_status(status: i32) {
    update(|probe| probe.creation_status = Some(status));
}
pub(super) fn created(window: &mut ExecutionWindow) {
    update(|probe| {
        probe.created += 1;
        if probe.expire_after_creation {
            // Only the test's local monotonic window changes. The real native
            // subscription and destructor are executed without replacing FFI.
            window.monotonic_deadline = Instant::now();
        }
    });
}
pub(super) fn callback(flags: Option<u32>, error: i32) {
    update(|probe| {
        if probe.callbacks.len() < MAX_ADDRESSES * 4 + 1 {
            probe.callbacks.push((flags, error));
        }
    });
}
pub(super) fn deallocated() {
    update(|probe| probe.deallocated += 1);
}

fn window(deadline: Instant) -> ExecutionWindow {
    let now = wall_now();
    ExecutionWindow::new(
        now + 20_000,
        now,
        // Keep the overall window later than the native five-second DNS stage;
        // stage expiry can then report the observed negative without overriding
        // an actual caller deadline. Campaign-wide remaining time is unchanged.
        Duration::from_secs(20).min(deadline.saturating_duration_since(Instant::now())),
    )
    .unwrap()
}

fn transport_stop(deadline: Instant, pre_cancel: bool) -> (Value, bool) {
    let cancellation = CancellationToken::default();
    let mut window = window(deadline);
    let expected = if pre_cancel {
        cancellation.cancel();
        StopReason::Cancelled
    } else {
        window.monotonic_deadline = Instant::now();
        StopReason::Deadline
    };
    let url = "https://example.com/".to_owned();
    let ticket = RequestTicket {
        run: CollectionTicket {
            job_id: "00000000-0000-4000-8000-000000000001".into(),
            generation: 1,
            lease: "00000000-0000-4000-8000-000000000002".into(),
        },
        sequence: 0,
        url: url.clone(),
    };
    let input = CollectionInput {
        urls: vec![url],
        max_hops: 0,
        max_requests: 1,
        max_seconds: 20,
    };
    let observation = fetch(&ticket, &input, window, Instant::now(), &cancellation);
    let passed = matches!(observation.outcome, Outcome::Stopped { reason, head: None } if reason == expected)
        && observation.locally_quiescent
        && observation.phase == Phase::BeforeRequest
        && observation.resolved.is_none();
    let actual_reason = match observation.outcome {
        Outcome::Stopped { reason, .. } => Some(reason),
        Outcome::Complete { .. } => None,
    };
    (
        json!({"reason": actual_reason, "phase": observation.phase,
               "locally_quiescent": observation.locally_quiescent,
               "observed_wall_ms": observation.observed_wall_ms,
               "transport_elapsed_milliseconds": observation.elapsed_milliseconds}),
        passed,
    )
}

fn resolve_case(host: &str, deadline: Instant, expected: &str) -> (Value, bool) {
    let result = NativeResolver.resolve(host, &mut window(deadline), &CancellationToken::default());
    match result {
        Ok(candidates) => {
            let passed = expected == "success"
                && !candidates.authoritative_complete_set
                && candidates.method == "macos_dns_service_observed_batch"
                && !candidates.addresses.is_empty()
                && candidates.addresses.len() <= MAX_ADDRESSES
                && candidates
                    .addresses
                    .iter()
                    .all(|a| policy::public_ip(a.ip()) && a.port() == 443);
            (
                json!({"result":"resolved", "method": candidates.method,
                    "candidate_count": candidates.addresses.len(),
                    "authoritative_complete_set": candidates.authoritative_complete_set}),
                passed,
            )
        }
        Err(ResolverFailure::Stopped(reason)) => {
            let passed = (expected == "negative" && reason == StopReason::Network)
                || (expected == "deadline" && reason == StopReason::Deadline);
            (json!({"result":"stopped", "reason":reason}), passed)
        }
        Err(ResolverFailure::QuiescenceUnverified(_)) => {
            (json!({"result":"quiescence_unverified"}), false)
        }
    }
}

#[test]
#[ignore = "explicit fixed DNS-only native campaign; use scripts/test_native_collection_transport.py"]
fn native_macos_dns_campaign() {
    assert_eq!(std::env::var("EW_NATIVE_DNS_PROOF").as_deref(), Ok(OPT_IN));
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut all_passed = true;
    for (name, host, expected_subscriptions) in [
        ("transport_pre_cancel", "example.com", 0),
        ("transport_expired", "example.com", 0),
        ("native_success", "example.com", 1),
        ("native_negative", "ew-native-proof.invalid", 1),
        ("native_active_deadline", "example.com", 1),
    ] {
        let started = Instant::now();
        PROBE.with_borrow_mut(|probe| {
            *probe = Some(Probe {
                expire_after_creation: name == "native_active_deadline",
                ..Probe::default()
            })
        });
        let (outcome, mut passed) = if started >= deadline {
            (
                json!({"result":"not_attempted", "reason":"campaign_deadline"}),
                false,
            )
        } else {
            match name {
                "transport_pre_cancel" => transport_stop(deadline, true),
                "transport_expired" => transport_stop(deadline, false),
                "native_success" => resolve_case(host, deadline, "success"),
                "native_negative" => resolve_case(host, deadline, "negative"),
                _ => resolve_case(host, deadline, "deadline"),
            }
        };
        let probe = PROBE.with_borrow_mut(|probe| probe.take().unwrap());
        passed &= probe.attempted == expected_subscriptions
            && probe.created == expected_subscriptions
            && probe.deallocated == expected_subscriptions;
        if name == "native_negative" {
            // kDNSServiceErr_NoSuchRecord, not a generic timeout/daemon failure.
            passed &= probe.callbacks.iter().any(|(_, error)| *error == -65554);
        }
        let event = json!({"case":name, "hostname":host, "passed":passed,
            "elapsed_milliseconds":started.elapsed().as_millis(),
            "probe":probe, "outcome":outcome});
        println!("{PREFIX}{event}");
        all_passed &= passed;
    }
    assert!(
        all_passed,
        "native observations did not satisfy every required case"
    );
}

#[test]
fn native_probe_is_thread_local_and_inactive_by_default() {
    attempted();
    callback(None, -65554);
    assert!(PROBE.with_borrow(Option::is_none));
    PROBE.with_borrow_mut(|probe| *probe = Some(Probe::default()));
    std::thread::spawn(attempted).join().unwrap();
    assert_eq!(
        PROBE
            .with_borrow_mut(|probe| probe.take().unwrap())
            .attempted,
        0
    );
}
