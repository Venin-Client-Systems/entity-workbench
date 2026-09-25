//! Private, unactivated transport. No canonical writes or public dispatch hooks.
#![allow(dead_code)]
use crate::{
    collection::PAGE_BYTES,
    collection_jobs::{CollectionInput, RequestTicket},
    engines::CancellationToken,
    policy,
};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, TryLockError,
    },
    time::{Duration, Instant},
};
use url::Url;

mod resolver;
pub(crate) use resolver::{ResolvedCandidates, ResolverUncertainty};
use resolver::{Resolver, ResolverFailure};

const POLL: Duration = Duration::from_millis(20);
const MAX_ADDRESSES: usize = 64;
static LIVE_REQUEST: Mutex<()> = Mutex::new(());
static RECOVERY_REQUIRED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallerContextState {
    ReleasedAfterCompletion,
    RetainedPendingCompletion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    BeforeRequest,
    Pacing,
    Dns,
    ConnectTlsHeaders,
    Body,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Cancelled,
    Deadline,
    Timeout,
    ClockChanged,
    Policy,
    Network,
    BodyLimit,
    ResolverUnavailable,
    QuiescenceUnverified,
    RecoveryRequired,
    Busy,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResponseHead {
    pub status: u16,
    pub media_type: Option<String>,
    pub redirect_url: Option<String>,
    pub identity_encoding: bool,
}
#[derive(Debug)]
pub(crate) enum Outcome {
    Complete {
        head: ResponseHead,
        body: Vec<u8>,
    },
    Stopped {
        reason: StopReason,
        head: Option<ResponseHead>,
    },
}
#[derive(Debug)]
pub(crate) struct Observation {
    pub outcome: Outcome,
    pub phase: Phase,
    pub elapsed_milliseconds: u64,
    pub observed_wall_ms: i64,
    pub resolved: Option<ResolvedCandidates>,
    pub resolver_uncertainty: Option<ResolverUncertainty>,
    /// A complete body can race cancellation/expiry; keep the bytes and this stop
    /// observation rather than falsely converting either into an ordinary success.
    pub stop_observed: Option<StopReason>,
    /// Local owned operation ended; says nothing about a server or shared DNS daemon.
    pub locally_quiescent: bool,
}

/// New execution segment, not a replacement for the canonical first-start deadline.
pub(crate) struct ExecutionWindow {
    wall_deadline_ms: i64,
    monotonic_deadline: Instant,
    previous_wall_ms: i64,
    last_observed_wall_ms: i64,
    wall_now: fn() -> i64,
}
fn wall_now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
impl ExecutionWindow {
    pub(crate) fn new(
        wall_deadline_ms: i64,
        previous_wall_ms: i64,
        remaining: Duration,
    ) -> Result<Self, StopReason> {
        if remaining > Duration::from_secs(600) || wall_deadline_ms < 0 || previous_wall_ms < 0 {
            return Err(StopReason::Policy);
        }
        Ok(Self {
            wall_deadline_ms,
            monotonic_deadline: Instant::now() + remaining,
            previous_wall_ms,
            last_observed_wall_ms: previous_wall_ms,
            wall_now,
        })
    }
    fn check(&mut self, cancellation: &CancellationToken) -> Result<(), StopReason> {
        let observed = (self.wall_now)();
        self.last_observed_wall_ms = observed;
        if observed < self.previous_wall_ms {
            return Err(StopReason::ClockChanged);
        }
        self.previous_wall_ms = observed;
        if cancellation.is_cancelled() {
            return Err(StopReason::Cancelled);
        }
        if observed >= self.wall_deadline_ms || Instant::now() >= self.monotonic_deadline {
            return Err(StopReason::Deadline);
        }
        Ok(())
    }
    fn remaining(&self) -> Duration {
        self.monotonic_deadline
            .saturating_duration_since(Instant::now())
    }
}

/// The future is borrowed across short waits; a wait timeout does not restart it.
async fn wait<F: Future>(
    future: F,
    window: &mut ExecutionWindow,
    cancellation: &CancellationToken,
    stage_deadline: Instant,
) -> Result<F::Output, StopReason> {
    let mut future = std::pin::pin!(future);
    loop {
        window.check(cancellation)?;
        if Instant::now() >= stage_deadline {
            return Err(StopReason::Timeout);
        }
        let delay = POLL
            .min(window.remaining())
            .min(stage_deadline.saturating_duration_since(Instant::now()));
        if let Ok(result) = tokio::time::timeout(delay, &mut future).await {
            return Ok(result);
        }
    }
}

/// Only the vetted host/address set can reach the HTTP connector. No GAI fallback.
struct PinnedResolver {
    host: String,
    addresses: Vec<SocketAddr>,
}
impl reqwest::dns::Resolve for PinnedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let addresses = self.addresses.clone();
        let permitted = name.as_str() == self.host;
        Box::pin(async move {
            if !permitted {
                return Err(std::io::Error::other("Unpinned DNS name refused").into());
            }
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

struct Configuration<'a> {
    recovery_required: &'a AtomicBool,
    #[cfg(test)]
    endpoint: Option<tests::TestEndpoint>,
}
impl Default for Configuration<'static> {
    fn default() -> Self {
        Self {
            recovery_required: &RECOVERY_REQUIRED,
            #[cfg(test)]
            endpoint: None,
        }
    }
}
impl Configuration<'_> {
    fn body_started(&self) {
        #[cfg(test)]
        if let Some(sender) = self.endpoint.as_ref().and_then(|e| e.body_started.as_ref()) {
            let _ = sender.send(());
        }
    }
    fn body_complete(&self) {
        #[cfg(test)]
        if let Some(cancel) = self
            .endpoint
            .as_ref()
            .and_then(|e| e.cancel_on_complete.as_ref())
        {
            cancel.cancel();
        }
    }
    fn target(
        &self,
        url: Url,
        pins: Vec<SocketAddr>,
    ) -> Result<(Url, Vec<SocketAddr>), StopReason> {
        #[cfg(test)]
        if let Some(endpoint) = &self.endpoint {
            let mut url = url;
            url.set_port(Some(endpoint.address.port()))
                .map_err(|_| StopReason::Policy)?;
            return Ok((url, vec![endpoint.address]));
        }
        Ok((url, pins))
    }
    fn client(&self, builder: reqwest::ClientBuilder) -> Result<reqwest::Client, StopReason> {
        #[cfg(test)]
        let builder = if let Some(endpoint) = &self.endpoint {
            if endpoint.trust_fixture_ca {
                builder.add_root_certificate(
                    reqwest::Certificate::from_der(&endpoint.ca).map_err(|_| StopReason::Policy)?,
                )
            } else {
                builder
            }
        } else {
            builder
        };
        builder.build().map_err(|_| StopReason::Network)
    }
}

/// Caller must obtain this private ticket from a committed canonical reservation.
/// This foundation has no production caller and never activates synthetic records.
pub(crate) fn fetch(
    ticket: &RequestTicket,
    input: &CollectionInput,
    window: ExecutionWindow,
    not_before: Instant,
    cancellation: &CancellationToken,
) -> Observation {
    if RECOVERY_REQUIRED.load(Ordering::Acquire) {
        return unstarted(StopReason::RecoveryRequired);
    }
    let _owned = match LIVE_REQUEST.try_lock() {
        Ok(guard) => guard,
        Err(error) => {
            let reason = match error {
                TryLockError::WouldBlock => StopReason::Busy,
                TryLockError::Poisoned(_) => {
                    RECOVERY_REQUIRED.store(true, Ordering::Release);
                    StopReason::RecoveryRequired
                }
            };
            return unstarted(reason);
        }
    };
    execute(
        ticket,
        input,
        window,
        not_before,
        cancellation,
        &resolver::NativeResolver,
        &Configuration::default(),
    )
}

fn unstarted(reason: StopReason) -> Observation {
    Observation {
        outcome: Outcome::Stopped { reason, head: None },
        phase: Phase::BeforeRequest,
        elapsed_milliseconds: 0,
        observed_wall_ms: wall_now(),
        resolved: None,
        resolver_uncertainty: None,
        stop_observed: None,
        locally_quiescent: reason == StopReason::Busy,
    }
}

fn execute(
    ticket: &RequestTicket,
    input: &CollectionInput,
    mut window: ExecutionWindow,
    not_before: Instant,
    cancellation: &CancellationToken,
    resolver: &dyn Resolver,
    configuration: &Configuration<'_>,
) -> Observation {
    let started = Instant::now();
    let mut phase = Phase::BeforeRequest;
    let mut resolved = None;
    let mut resolver_uncertainty = None;
    let mut head = None;
    let result = (|| {
        if configuration.recovery_required.load(Ordering::Acquire) {
            return Err(StopReason::RecoveryRequired);
        }
        window.check(cancellation)?;
        let normalized = input.clone().normalized().map_err(|_| StopReason::Policy)?;
        let url = policy::validate_https_url(&ticket.url).map_err(|_| StopReason::Policy)?;
        if url.as_str() != ticket.url {
            return Err(StopReason::Policy);
        }
        let host = url.host_str().ok_or(StopReason::Policy)?;
        let selected: Vec<_> = normalized
            .urls
            .iter()
            .map(|u| Url::parse(u).expect("normalized seed"))
            .collect();
        let hosts: Vec<_> = selected.iter().filter_map(Url::host_str).collect();
        if !hosts.contains(&host) {
            return Err(StopReason::Policy);
        }
        phase = Phase::Pacing;
        while Instant::now() < not_before {
            window.check(cancellation)?;
            std::thread::sleep(
                POLL.min(not_before.saturating_duration_since(Instant::now()))
                    .min(window.remaining()),
            );
        }
        phase = Phase::Dns;
        let addresses = resolver
            .resolve(host, &mut window, cancellation)
            .map_err(|failure| match failure {
                ResolverFailure::Stopped(reason) => reason,
                ResolverFailure::QuiescenceUnverified(uncertainty) => {
                    resolver_uncertainty = Some(uncertainty);
                    StopReason::QuiescenceUnverified
                }
            })?;
        if addresses.addresses.is_empty() || addresses.addresses.len() > MAX_ADDRESSES {
            return Err(StopReason::Policy);
        }
        policy::validate_destination(
            url.as_str(),
            &addresses
                .addresses
                .iter()
                .map(|a| a.ip())
                .collect::<Vec<_>>(),
            &hosts,
        )
        .map_err(|_| StopReason::Policy)?;
        if addresses.addresses.iter().any(|a| a.port() != 443) {
            return Err(StopReason::Policy);
        }
        let pins = addresses.addresses.clone();
        resolved = Some(addresses);
        window.check(cancellation)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| StopReason::Network)?;
        let result = runtime.block_on(http(
            url,
            pins,
            &mut window,
            cancellation,
            configuration,
            &mut phase,
            &mut head,
        ));
        // No GAI/spawn_blocking work is permitted. Dropping this dedicated runtime
        // closes owned async tasks/connections before acknowledging local stop.
        drop(runtime);
        result
    })();
    if matches!(result, Err(StopReason::QuiescenceUnverified)) {
        configuration
            .recovery_required
            .store(true, Ordering::Release);
    }
    let locally_quiescent = !matches!(
        result,
        Err(StopReason::QuiescenceUnverified | StopReason::RecoveryRequired)
    );
    let stop_observed = window.check(cancellation).err();
    Observation {
        outcome: match result {
            Ok(body) => Outcome::Complete {
                head: head.expect("complete response has headers"),
                body,
            },
            Err(reason) => Outcome::Stopped { reason, head },
        },
        phase,
        elapsed_milliseconds: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        observed_wall_ms: window.last_observed_wall_ms,
        resolved,
        resolver_uncertainty,
        stop_observed,
        locally_quiescent,
    }
}

async fn http(
    url: Url,
    pins: Vec<SocketAddr>,
    window: &mut ExecutionWindow,
    cancellation: &CancellationToken,
    configuration: &Configuration<'_>,
    phase: &mut Phase,
    head: &mut Option<ResponseHead>,
) -> Result<Vec<u8>, StopReason> {
    // Relative Location is interpreted against the selected source URL, never
    // the cfg(test) loopback port used to exercise the same connector.
    let source_url = url.clone();
    let (url, pins) = configuration.target(url, pins)?;
    let request_deadline = Instant::now() + Duration::from_secs(15).min(window.remaining());
    let builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .https_only(true)
        .http1_only()
        .retry(reqwest::retry::never())
        .pool_max_idle_per_host(0)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .connect_timeout(Duration::from_secs(5).min(window.remaining()))
        .timeout(Duration::from_secs(15).min(window.remaining()))
        .dns_resolver(Arc::new(PinnedResolver {
            host: url.host_str().ok_or(StopReason::Policy)?.into(),
            addresses: pins.clone(),
        }))
        .user_agent("EntityWorkbench/0.1 (analyst-directed public collection)");
    let client = configuration.client(builder)?;
    *phase = Phase::ConnectTlsHeaders;
    let mut response = wait(
        client
            .get(url.as_str())
            .header("Accept", "text/html, text/plain;q=0.9")
            .header("Accept-Encoding", "identity")
            .send(),
        window,
        cancellation,
        request_deadline,
    )
    .await?
    .map_err(|_| StopReason::Network)?;
    if !response
        .remote_addr()
        .is_some_and(|address| pins.contains(&address))
    {
        return Err(StopReason::Policy);
    }
    let media_type = response
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .and_then(crate::collection::media_type);
    let redirect_url = response
        .headers()
        .get("location")
        .and_then(|h| h.to_str().ok())
        .filter(|h| h.len() <= 2048)
        .and_then(|h| source_url.join(h).ok())
        .and_then(|u| policy::validate_https_url(u.as_str()).ok())
        .map(|u| u.to_string());
    let identity_encoding = response
        .headers()
        .get_all("content-encoding")
        .iter()
        .all(|h| {
            h.to_str()
                .is_ok_and(|v| v.trim().eq_ignore_ascii_case("identity"))
        });
    *head = Some(ResponseHead {
        status: response.status().as_u16(),
        media_type,
        redirect_url,
        identity_encoding,
    });
    *phase = Phase::Body;
    configuration.body_started();
    if response
        .content_length()
        .is_some_and(|bytes| bytes > PAGE_BYTES as u64)
    {
        return Err(StopReason::BodyLimit);
    }
    let mut body = Vec::new();
    loop {
        let chunk = wait(response.chunk(), window, cancellation, request_deadline)
            .await?
            .map_err(|_| StopReason::Network)?;
        let Some(chunk) = chunk else {
            configuration.body_complete();
            break;
        };
        if chunk.len() > PAGE_BYTES - body.len() {
            return Err(StopReason::BodyLimit);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
pub(crate) mod tests;
