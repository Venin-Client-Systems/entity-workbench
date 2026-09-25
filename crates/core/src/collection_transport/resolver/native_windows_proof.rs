//! Test-only observation of the real GetAddrInfoExW path. No DNS API replacement.
use super::*;
use crate::collection_jobs::CollectionTicket;
use crate::collection_transport::{fetch, Outcome};
use serde_json::{json, Value};
use std::cell::RefCell;
#[cfg(target_os = "windows")]
use std::io::Write;

const POLICY: &str = "fixed-three-windows-lookups-no-http-v1";
const BUILD_SOURCE: Option<&str> = option_env!("EW_WINDOWS_DNS_BUILD_SOURCE");
const PREFIX: &str = "EW_WINDOWS_DNS_CASE=";
const RETURN_LIMIT: usize = 64;

#[derive(Default, serde::Serialize)]
struct Probe {
    wsa_startup: Vec<i32>,
    events_created: u32,
    contexts_created: u32,
    launch_returns: Vec<i32>,
    cancel_returns: Vec<i32>,
    completion_returns: Vec<i32>,
    wait_signalled: u32,
    wait_timeouts: u32,
    wait_errors: Vec<u32>,
    contexts_dropped: u32,
    contexts_retained: u32,
    results_freed: u32,
    event_close: Vec<bool>,
    wsa_cleanup: Vec<i32>,
    trace_truncated: bool,
    #[serde(skip)]
    cancel_after_launch: bool,
}
thread_local! {
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
}
fn update(f: impl FnOnce(&mut Probe)) {
    PROBE.with_borrow_mut(|state| {
        if let Some(probe) = state {
            f(probe);
        }
    });
}
fn bounded_push<T>(values: &mut Vec<T>, value: T, truncated: &mut bool) {
    if values.len() < RETURN_LIMIT {
        values.push(value);
    } else {
        *truncated = true;
    }
}
pub(super) fn wsa_startup(value: i32) {
    update(|p| bounded_push(&mut p.wsa_startup, value, &mut p.trace_truncated));
}
pub(super) fn event_created() {
    update(|p| p.events_created += 1);
}
pub(super) fn context_created() {
    update(|p| p.contexts_created += 1);
}
pub(super) fn launched(value: i32, token: &CancellationToken) {
    update(|p| {
        bounded_push(&mut p.launch_returns, value, &mut p.trace_truncated);
        if p.cancel_after_launch {
            // The actual OS call already returned. This does not force an async
            // return, replace FFI, manufacture completion or touch caller buffers.
            token.cancel();
        }
    });
}
pub(super) fn cancel_returned(value: i32) {
    update(|p| bounded_push(&mut p.cancel_returns, value, &mut p.trace_truncated));
}
pub(super) fn completion_returned(value: i32) {
    update(|p| bounded_push(&mut p.completion_returns, value, &mut p.trace_truncated));
}
pub(super) fn wait_returned(value: u32) {
    update(|p| match value {
        0 => p.wait_signalled = p.wait_signalled.saturating_add(1),
        258 => p.wait_timeouts = p.wait_timeouts.saturating_add(1),
        other => bounded_push(&mut p.wait_errors, other, &mut p.trace_truncated),
    });
}
pub(super) fn context_dropped() {
    update(|p| p.contexts_dropped += 1);
}
pub(super) fn context_retained() {
    update(|p| p.contexts_retained += 1);
}
pub(super) fn result_freed() {
    update(|p| p.results_freed += 1);
}
pub(super) fn event_closed(value: bool) {
    update(|p| bounded_push(&mut p.event_close, value, &mut p.trace_truncated));
}
pub(super) fn wsa_cleanup(value: i32) {
    update(|p| bounded_push(&mut p.wsa_cleanup, value, &mut p.trace_truncated));
}
fn start_probe(cancel_after_launch: bool) {
    PROBE.with_borrow_mut(|p| {
        *p = Some(Probe {
            cancel_after_launch,
            ..Probe::default()
        })
    });
}
fn take_probe() -> Probe {
    PROBE.with_borrow_mut(|p| p.take().unwrap())
}
fn window(deadline: Instant) -> ExecutionWindow {
    let now = wall_now();
    ExecutionWindow::new(
        now + 20_000,
        now,
        deadline.saturating_duration_since(Instant::now()),
    )
    .unwrap()
}
fn input(host: &str) -> (RequestTicket, CollectionInput) {
    let url = format!("https://{host}/");
    (
        RequestTicket {
            run: CollectionTicket {
                job_id: "00000000-0000-4000-8000-000000000001".into(),
                generation: 1,
                lease: "00000000-0000-4000-8000-000000000002".into(),
            },
            sequence: 0,
            url: url.clone(),
        },
        CollectionInput {
            urls: vec![url],
            max_hops: 0,
            max_requests: 1,
            max_seconds: 20,
        },
    )
}
fn observation(value: Observation) -> Value {
    let reason = match value.outcome {
        Outcome::Stopped { reason, head: None } => Some(reason),
        _ => None,
    };
    json!({"kind":"transport", "reason":reason, "phase":value.phase,
        "locally_quiescent":value.locally_quiescent,
        "caller_context":value.resolver_uncertainty.map(|u| u.caller_context),
        "stop_observed":value.stop_observed})
}
fn released(probe: &Probe) -> bool {
    probe.contexts_created == 1
        && probe.contexts_dropped == 1
        && probe.contexts_retained == 0
        && probe.event_close == [true]
        && probe.wsa_cleanup == [0]
        && !probe.trace_truncated
}
fn zero_launch(probe: &Probe) -> bool {
    probe.wsa_startup.is_empty()
        && probe.events_created == 0
        && probe.contexts_created == 0
        && probe.launch_returns.is_empty()
        && probe.contexts_dropped == 0
        && probe.contexts_retained == 0
        && probe.event_close.is_empty()
        && probe.wsa_cleanup.is_empty()
}
fn prelaunch(deadline: Instant, cancelled: bool) -> (Value, bool) {
    let (ticket, input) = input("example.com");
    let token = CancellationToken::default();
    let mut window = window(deadline);
    let expected = if cancelled {
        token.cancel();
        "cancelled"
    } else {
        window.monotonic_deadline = Instant::now();
        "deadline"
    };
    let value = observation(fetch(&ticket, &input, window, Instant::now(), &token));
    let passed = value["reason"] == expected
        && value["phase"] == "before_request"
        && value["locally_quiescent"] == true;
    (value, passed)
}
fn lookup(host: &str, deadline: Instant, success: bool) -> (Value, bool, bool) {
    match NativeResolver.resolve(host, &mut window(deadline), &CancellationToken::default()) {
        Ok(value) => {
            let passed = success
                && !value.authoritative_complete_set
                && value.method == "windows_completed_system_candidates"
                && !value.addresses.is_empty()
                && value.addresses.len() <= MAX_ADDRESSES
                && value
                    .addresses
                    .iter()
                    .all(|a| policy::public_ip(a.ip()) && a.port() == 443);
            (
                json!({"kind":"resolved", "candidate_count":value.addresses.len(),
                "method":value.method, "authoritative_complete_set":value.authoritative_complete_set}),
                passed,
                false,
            )
        }
        Err(ResolverFailure::Stopped(reason)) => (
            json!({"kind":"stopped", "reason":reason}),
            !success && reason == StopReason::Network,
            false,
        ),
        Err(ResolverFailure::QuiescenceUnverified(value)) => (
            json!({"kind":"quiescence_unverified", "caller_context":value.caller_context}),
            false,
            true,
        ),
    }
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "explicit source-bound Windows DNS-only campaign; see dedicated runner"]
fn native_windows_dns_campaign() {
    assert_eq!(std::env::var("EW_WINDOWS_DNS_PROOF").as_deref(), Ok(POLICY));
    let source = std::env::var("EW_WINDOWS_DNS_SOURCE").unwrap();
    assert!(
        source.len() == 40
            && source
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert_eq!(BUILD_SOURCE, Some(source.as_str()));
    let nonce = std::env::var("EW_WINDOWS_DNS_NONCE").unwrap();
    assert_eq!(uuid::Uuid::parse_str(&nonce).unwrap().to_string(), nonce);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut passed_all = true;
    let mut stop_campaign = false;
    for (name, host) in [
        ("transport_pre_cancel", "example.com"),
        ("transport_expired", "example.com"),
        ("native_success", "example.com"),
        ("native_negative", "ew-native-proof.invalid"),
        ("native_pending_cancel", "ew-native-cancel-proof.invalid"),
    ] {
        let started = Instant::now();
        start_probe(name == "native_pending_cancel");
        let mut followup = Value::Null;
        let mut state = "observed";
        let (outcome, mut passed) = if stop_campaign || started >= deadline {
            state = "not_attempted";
            (json!({"kind":"not_attempted"}), false)
        } else if name == "transport_pre_cancel" || name == "transport_expired" {
            prelaunch(deadline, name == "transport_pre_cancel")
        } else if name == "native_success" || name == "native_negative" {
            let (value, passed, uncertain) = lookup(host, deadline, name == "native_success");
            stop_campaign = uncertain;
            (value, passed)
        } else {
            let (ticket, input) = input(host);
            let value = observation(fetch(
                &ticket,
                &input,
                window(deadline),
                Instant::now(),
                &CancellationToken::default(),
            ));
            let passed =
                value["reason"] == "quiescence_unverified" && value["locally_quiescent"] == false;
            (value, passed)
        };
        let probe = take_probe();
        if state != "not_attempted" {
            if name.starts_with("transport_") {
                passed &= zero_launch(&probe);
            } else if name != "native_pending_cancel" {
                passed &= released(&probe);
                if name == "native_success" {
                    passed &= probe.results_freed == 1;
                } else {
                    passed &= probe
                        .launch_returns
                        .iter()
                        .chain(&probe.completion_returns)
                        .any(|code| matches!(code, 11001 | 11004));
                }
            } else if probe.launch_returns != [997] {
                state = "not_exercised";
                passed = false; // Never repeat/force asynchronous completion.
            } else {
                let caller = outcome["caller_context"].as_str();
                passed &= !probe.cancel_returns.is_empty() && !probe.trace_truncated;
                passed &= match caller {
                    Some("released_after_completion") => {
                        released(&probe)
                            && probe
                                .completion_returns
                                .last()
                                .is_some_and(|code| !windows_result_pending(*code))
                    }
                    Some("retained_pending_completion") => {
                        probe.contexts_retained == 1
                            && probe.contexts_dropped == 0
                            && probe.event_close.is_empty()
                            && probe.wsa_cleanup.is_empty()
                            && probe.results_freed == 0
                    }
                    _ => false,
                };
                start_probe(false);
                let (ticket, input) = input(host);
                let stopped = observation(fetch(
                    &ticket,
                    &input,
                    window(deadline),
                    Instant::now(),
                    &CancellationToken::default(),
                ));
                let refused_probe = take_probe();
                passed &= stopped["reason"] == "recovery_required"
                    && stopped["locally_quiescent"] == false
                    && zero_launch(&refused_probe);
                followup = json!({"outcome":stopped,"probe":refused_probe});
            }
        }
        let value = json!({"schema_version":1, "source_commit":source,
            "build_source_commit":BUILD_SOURCE, "nonce":nonce, "policy":POLICY,
            "case":name,"hostname":host,"state":state,"passed":passed,
            "elapsed_milliseconds":started.elapsed().as_millis(),"outcome":outcome,
            "probe":probe,"followup":followup});
        println!("{PREFIX}{value}");
        std::io::stdout().flush().unwrap();
        passed_all &= passed;
    }
    assert!(
        passed_all,
        "native Windows DNS campaign was incomplete or failed"
    );
}

#[test]
fn probe_is_inactive_by_default_and_bounded_without_global_callback_state() {
    wsa_startup(0);
    assert!(PROBE.with_borrow(Option::is_none));
    start_probe(false);
    std::thread::spawn(|| wsa_startup(0)).join().unwrap();
    for _ in 0..=RETURN_LIMIT {
        completion_returned(10036);
    }
    let probe = take_probe();
    assert!(probe.wsa_startup.is_empty());
    assert!(probe.trace_truncated);
    assert_eq!(probe.completion_returns.len(), RETURN_LIMIT);
}

#[test]
fn launch_hook_only_cancels_its_owned_token_after_explicit_test_request() {
    start_probe(false);
    let token = CancellationToken::default();
    launched(997, &token);
    assert!(!token.is_cancelled());
    take_probe();
    start_probe(true);
    launched(0, &token);
    assert!(token.is_cancelled());
    assert_eq!(take_probe().launch_returns, [0]);
}
