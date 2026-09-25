//! Test-only, per-capability native proof observation. No global hook or worker IPC.
use super::*;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct State {
    calls: u32,
    launches: u32,
    pid: Option<u32>,
    launch_ms: Option<u64>,
    live_ms: Option<u64>,
    cancel_seen_ms: Option<u64>,
    stop_ms: Option<u64>,
    termination: &'static str,
    cleanup: &'static str,
    profile_sha256: Option<String>,
    assigned: Option<serde_json::Value>,
    pre_inventory: bool,
    post_inventory: bool,
    job_id: Option<String>,
    nonce: Option<String>,
    request: Vec<u8>,
    wrapper: Vec<u8>,
    result: Vec<u8>,
}

pub(crate) struct TestObservation {
    state: Mutex<State>,
    changed: Condvar,
    start: Instant,
    cancel_after_live: bool,
}
impl TestObservation {
    pub(crate) fn new(cancel_after_live: bool) -> Self {
        Self {
            state: Mutex::new(State {
                termination: "not_started",
                cleanup: "not_started",
                ..State::default()
            }),
            changed: Condvar::new(),
            start: Instant::now(),
            cancel_after_live,
        }
    }
    fn elapsed(&self) -> u64 {
        self.start
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(super) fn assignment(&self, id: &str, nonce: &str, request: &[u8]) -> Result<()> {
        let mut state = self.state();
        state.calls += 1;
        require(
            state.calls == 1 && request.len() <= REQUEST_LIMIT,
            "Native proof executor repeated or input excessive",
        )?;
        state.job_id = Some(id.into());
        state.nonce = Some(nonce.into());
        state.request = request.to_vec();
        Ok(())
    }
    pub(crate) fn prepared(&self, profile: &str, assigned: serde_json::Value) -> Result<()> {
        require(
            profile.len() <= 64 * 1024 && assigned.as_object().is_some_and(|v| v.len() == 6),
            "Native proof prepared assets differ",
        )?;
        let mut state = self.state();
        require(
            state.launches == 0 && state.assigned.is_none(),
            "Native proof preparation repeated",
        )?;
        state.profile_sha256 = Some(digest(profile.as_bytes()));
        state.assigned = Some(assigned);
        Ok(())
    }
    pub(crate) fn inventory(&self, after: bool) {
        let mut state = self.state();
        if after {
            state.post_inventory = true;
        } else {
            state.pre_inventory = true;
        }
    }
    pub(crate) fn launched(&self, pid: u32) -> Result<()> {
        let mut state = self.state();
        state.launches += 1;
        state.pid = Some(pid);
        state.launch_ms = Some(self.elapsed());
        state.termination = "unconfirmed";
        require(state.launches == 1, "Native proof launched more than once")
    }
    // This call is inside the wait outcome closure, after ProcessGroup ownership.
    // Errors still flow through the same stop/reap path. No coordinator lock is held.
    pub(crate) fn alive(&self, pid: u32, token: Option<&CancellationToken>) -> Result<()> {
        let mut state = self.state();
        require(
            state.pid == Some(pid),
            "Native proof child identity differs",
        )?;
        if state.live_ms.is_some() {
            return Ok(());
        }
        state.live_ms = Some(self.elapsed());
        self.changed.notify_all();
        if !self.cancel_after_live {
            return Ok(());
        }
        let token = token
            .ok_or_else(|| Error::Validation("Native proof cancellation owner missing".into()))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !token.is_cancelled() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    Error::Validation("Native proof cancellation handshake timed out".into())
                })?;
            let (next, _) = self
                .changed
                .wait_timeout(state, remaining.min(Duration::from_millis(20)))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
        }
        state.cancel_seen_ms = Some(self.elapsed());
        Ok(())
    }
    pub(crate) fn stopped(&self, confirmed: bool) {
        let mut state = self.state();
        state.termination = if confirmed { "confirmed" } else { "unverified" };
        state.stop_ms = Some(self.elapsed());
        self.changed.notify_all();
    }
    pub(super) fn accepted(&self, wrapper: &[u8], result: &[u8]) -> Result<()> {
        let mut state = self.state();
        require(
            state.termination == "confirmed"
                && wrapper.len() <= WRAPPER_LIMIT
                && result.len() <= RESULT_LIMIT,
            "Native proof output cannot be retained before confirmed stop",
        )?;
        state.wrapper = wrapper.to_vec();
        state.result = result.to_vec();
        Ok(())
    }
    pub(crate) fn cleaned<T>(&self, outcome: &Result<T>) {
        let mut state = self.state();
        state.cleanup = match outcome {
            Err(Error::TerminationUnverified(_)) => "not_attempted_unverified",
            Err(Error::Cleanup(_)) => "failed",
            _ => "confirmed",
        };
        self.changed.notify_all();
    }
    pub(crate) fn wait_live(&self, duration: Duration) -> Result<()> {
        let deadline = Instant::now() + duration;
        let mut state = self.state();
        while state.live_ms.is_none() {
            require(
                !matches!(state.termination, "confirmed" | "unverified"),
                "No live-child observation; no replacement launch allowed",
            )?;
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    Error::Validation("Native proof did not observe live child".into())
                })?;
            state = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        Ok(())
    }
    pub(crate) fn receipt(&self) -> serde_json::Value {
        let state = self.state();
        serde_json::json!({"execution_calls":state.calls,"launch_count":state.launches,"pid":state.pid,
            "launch_ms":state.launch_ms,"live_observed_ms":state.live_ms,"cancellation_seen_ms":state.cancel_seen_ms,
            "stop_ms":state.stop_ms,"termination":state.termination,"cleanup":state.cleanup,
            "profile_sha256":state.profile_sha256,"assigned_files":state.assigned,
            "pre_inventory_verified":state.pre_inventory,"post_inventory_verified":state.post_inventory,
            "assignment_id":state.job_id,"capture_nonce":state.nonce,
            "request_identity":Identity::of(&state.request),"wrapper_identity":Identity::of(&state.wrapper),
            "result_identity":Identity::of(&state.result)})
    }
    pub(crate) fn retained(&self) -> Result<[Vec<u8>; 3]> {
        let state = self.state();
        require(
            state.termination == "confirmed" && state.cleanup == "confirmed",
            "Native proof output unavailable",
        )?;
        Ok([
            state.request.clone(),
            state.wrapper.clone(),
            state.result.clone(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_is_per_assignment_and_never_exposes_unconfirmed_outputs() {
        let a = TestObservation::new(false);
        let b = TestObservation::new(false);
        a.assignment("id", "nonce", b"request").unwrap();
        a.launched(123).unwrap();
        assert!(a.retained().is_err());
        a.stopped(false);
        a.cleaned::<()>(&Err(Error::TerminationUnverified("test".into())));
        assert!(a.retained().is_err());
        assert_eq!(a.receipt()["cleanup"], "not_attempted_unverified");
        assert_eq!(b.receipt()["launch_count"], 0);
    }
    #[test]
    fn observed_live_is_distinct_from_launch_and_repeated_execution_is_visible() {
        let a = TestObservation::new(false);
        a.assignment("id", "nonce", b"request").unwrap();
        a.launched(123).unwrap();
        assert!(a.receipt()["live_observed_ms"].is_null());
        a.alive(123, None).unwrap();
        a.stopped(true);
        a.accepted(b"wrapper", b"result").unwrap();
        a.cleaned(&Ok(()));
        assert_eq!(a.retained().unwrap()[2], b"result");
        assert!(a.assignment("other", "nonce", b"request").is_err());
        assert_eq!(a.receipt()["execution_calls"], 2);
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[test]
    fn cancellation_handshake_waits_for_caller_token_after_observed_live() {
        let observer = std::sync::Arc::new(TestObservation::new(true));
        observer.assignment("id", "nonce", b"request").unwrap();
        observer.launched(123).unwrap();
        let child = observer.clone();
        let token = CancellationToken::default();
        let caller = token.clone();
        let thread = std::thread::spawn(move || child.alive(123, Some(&token)));
        observer.wait_live(Duration::from_secs(1)).unwrap();
        assert!(observer.receipt()["cancellation_seen_ms"].is_null());
        caller.cancel();
        thread.join().unwrap().unwrap();
        assert!(observer.receipt()["cancellation_seen_ms"].is_number());
        assert!(observer.retained().is_err()); // observing cancellation never proves termination
    }
}
