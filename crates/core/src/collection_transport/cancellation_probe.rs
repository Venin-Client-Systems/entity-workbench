//! Closed test coordination only. Ordinary cancellation still stops the next body wait.
use super::*;
use std::sync::mpsc::{self, Receiver, SyncSender};

const HANDSHAKE_LIMIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct GateState {
    pub reached_headers: bool,
    pub notification_failed: bool,
    pub cancellation_observed: bool,
    pub handshake_expired: bool,
}
pub(crate) struct ResponseGate {
    sender: SyncSender<ResponseHead>,
    state: Mutex<GateState>,
}
impl ResponseGate {
    pub(crate) fn new() -> (Self, Receiver<ResponseHead>) {
        let (sender, receiver) = mpsc::sync_channel(1);
        (
            Self {
                sender,
                state: Mutex::new(GateState::default()),
            },
            receiver,
        )
    }
    pub(crate) fn state(&self) -> GateState {
        self.state.lock().expect("owned response gate").clone()
    }
    pub(crate) async fn pause(
        &self,
        head: &ResponseHead,
        token: &CancellationToken,
        remaining: Duration,
    ) -> Result<(), StopReason> {
        let deadline = Instant::now() + HANDSHAKE_LIMIT.min(remaining);
        {
            let mut state = self.state.lock().expect("owned response gate");
            if state.reached_headers || self.sender.try_send(head.clone()).is_err() {
                state.notification_failed = true;
                return Err(StopReason::Policy);
            }
            state.reached_headers = true;
        }
        loop {
            if token.is_cancelled() {
                self.state
                    .lock()
                    .expect("owned response gate")
                    .cancellation_observed = true;
                // The hook never returns Cancelled or manufactures an observation.
                // The normal wait(response.chunk()) checks the real token before polling.
                return Ok(());
            }
            if Instant::now() >= deadline {
                self.state
                    .lock()
                    .expect("owned response gate")
                    .handshake_expired = true;
                // A failed test handshake stops without consuming the body or another request.
                return Err(StopReason::Timeout);
            }
            tokio::time::sleep(POLL.min(deadline.saturating_duration_since(Instant::now()))).await;
        }
    }
}

/// The normal global lane/quarantine, native resolver and verified pinned connector still apply.
#[cfg(target_os = "macos")]
pub(crate) fn fetch_with_gate(
    ticket: &RequestTicket,
    input: &CollectionInput,
    window: ExecutionWindow,
    pacing: Instant,
    token: &CancellationToken,
    gate: &ResponseGate,
) -> Observation {
    fetch_configured(
        ticket,
        input,
        window,
        pacing,
        token,
        &Configuration {
            response_gate: Some(gate),
            ..Default::default()
        },
    )
}
