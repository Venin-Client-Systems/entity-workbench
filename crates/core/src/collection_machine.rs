//! Deterministic, replay-validated collection transitions. No networking or writes.
#![allow(dead_code)]
use crate::{
    collection::{self, PAGE_BYTES},
    collection_jobs::*,
    policy, require, Error, Result,
};
use std::collections::BTreeMap;
use url::Url;

pub(crate) struct Promotion {
    pub sha256: String,
    pub text: String,
    pub media_type: String,
}
#[derive(Clone)]
pub(crate) struct Machine {
    pub checkpoint: CollectionCheckpoint,
    input: CollectionInput,
    hosts: Vec<String>,
    policies: BTreeMap<String, Option<String>>,
    version: u32,
    replaying: bool,
}
impl Machine {
    pub fn new(input: &CollectionInput, at_ms: i64) -> Result<Self> {
        Self::new_version(input, at_ms, 1, false)
    }
    pub(crate) fn new_version(
        input: &CollectionInput,
        at_ms: i64,
        version: u32,
        replaying: bool,
    ) -> Result<Self> {
        require(
            input.clone().normalized()? == *input,
            "Collection input is not canonical",
        )?;
        require(
            matches!(version, 1..=4),
            "Unsupported collection state machine version",
        )?;
        if matches!(version, 2..=4) && replaying {
            require(at_ms >= 0, "Invalid collection timestamp")?;
        } else {
            valid_time(at_ms)?;
        }
        let frontier = input
            .urls
            .iter()
            .map(|url| FrontierEntry {
                url: url.clone(),
                hop: 0,
                redirects: 0,
                purpose: Purpose::Seed,
                parent: None,
            })
            .collect();
        let hosts = input
            .urls
            .iter()
            .map(|raw| {
                Url::parse(raw)
                    .expect("validated URL")
                    .host_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        Ok(Self {
            version,
            replaying,
            input: input.clone(),
            hosts,
            policies: BTreeMap::new(),
            checkpoint: CollectionCheckpoint {
                state: CollectionState::Queued,
                generation: 1,
                lease: None,
                first_started_at_ms: None,
                deadline_at_ms: None,
                updated_at_ms: at_ms,
                cancellation_requested: false,
                frontier,
                visited: Vec::new(),
                robots: Vec::new(),
                requests: Vec::new(),
                pages_retained: 0,
                saw_blocked: false,
                saw_failed: false,
                saw_quota: false,
                saw_unknown: false,
            },
        })
    }
    pub fn apply(
        &mut self,
        event: &CollectionEvent,
        body: Option<&[u8]>,
    ) -> Result<Option<Promotion>> {
        if let CollectionEvent::TransportObserved {
            clock_anchor_ms,
            sequence,
            receipt,
        } = event
        {
            return self.observe_transport(*clock_anchor_ms, *sequence, receipt, body);
        }
        let at = event.at_ms();
        if matches!(self.version, 2..=4) && self.replaying {
            require(at >= 0, "Invalid collection timestamp")?;
        } else {
            valid_time(at)?;
        }
        self.apply_ordered_event(event, body)
    }

    /// Stop intent for an already reserved v2 request. The timestamp is the
    /// machine's own validated journal anchor, never an external clock sample.
    /// This remains usable after a rollback makes that anchor appear future-dated.
    pub(crate) fn cancel_reserved_at_checkpoint(&mut self) -> Result<CollectionEvent> {
        require(
            matches!(self.version, 2..=4)
                && self.checkpoint.state == CollectionState::Running
                && self
                    .checkpoint
                    .requests
                    .last()
                    .is_some_and(|request| matches!(request.progress, RequestProgress::Reserved)),
            "Anchored cancellation requires a running reserved v2 request",
        )?;
        let event = CollectionEvent::Cancel {
            at_ms: self.checkpoint.updated_at_ms,
        };
        self.apply_ordered_event(&event, None)?;
        Ok(event)
    }

    /// Reapply only a stored v4 cancellation suffix to a verified prefix. The
    /// caller must compare the resulting entire record with its canonical read.
    /// This uses historical time ordering, not the current OS wall clock.
    pub(crate) fn replay_cancel_suffix(&mut self, event: &CollectionEvent) -> Result<()> {
        require(
            self.version == 4 && matches!(event, CollectionEvent::Cancel { at_ms } if *at_ms >= 0),
            "Invalid prepared cancellation suffix",
        )?;
        self.apply_ordered_event(event, None)?;
        Ok(())
    }

    fn apply_ordered_event(
        &mut self,
        event: &CollectionEvent,
        body: Option<&[u8]>,
    ) -> Result<Option<Promotion>> {
        let at = event.at_ms();
        require(
            at >= self.checkpoint.updated_at_ms,
            "Collection clock moved backwards",
        )?;
        let mut promotion = None;
        match event {
            CollectionEvent::TransportObserved { .. } => unreachable!("handled above"),
            CollectionEvent::Start { lease, .. } => {
                require(
                    self.checkpoint.state == CollectionState::Queued && canonical_uuid(lease),
                    "Invalid collection start",
                )?;
                self.checkpoint.first_started_at_ms = Some(at);
                self.checkpoint.deadline_at_ms = Some(first_deadline(at, self.input.max_seconds)?);
                self.checkpoint.lease = Some(lease.clone());
                self.checkpoint.state = CollectionState::Running;
            }
            CollectionEvent::Resume { lease, .. } => {
                require(
                    self.checkpoint.state == CollectionState::Interrupted
                        && !self.checkpoint.cancellation_requested
                        && self.checkpoint.generation < 8
                        && canonical_uuid(lease),
                    "Collection cannot resume",
                )?;
                require(
                    self.checkpoint.requests.iter().all(|r| r.lease != *lease),
                    "Collection lease was reused",
                )?;
                self.checkpoint.generation += 1;
                self.checkpoint.lease = Some(lease.clone());
                self.checkpoint.state = CollectionState::Running;
                if self.expired(at) {
                    self.finish(CollectionState::QuotaExhausted);
                }
            }
            CollectionEvent::Advance { .. } => self.advance(at)?,
            CollectionEvent::Complete {
                sequence, result, ..
            } => {
                require(
                    self.version == 1,
                    "V2/v3/v4 require a lossless transport receipt",
                )?;
                self.running()?;
                let request = self
                    .checkpoint
                    .requests
                    .get(*sequence as usize)
                    .ok_or_else(|| Error::Validation("Unknown charged request".into()))?
                    .clone();
                require(
                    request.sequence == *sequence
                        && matches!(request.progress, RequestProgress::Reserved),
                    "Request is not reserved",
                )?;
                validate_fetch(result, body)?;
                promotion = self.received(&request, result, body, at)?;
                self.checkpoint.requests[*sequence as usize].progress = RequestProgress::Settled {
                    ended_at_ms: at,
                    result: result.clone(),
                };
            }
            CollectionEvent::Cancel { .. } => {
                require(
                    matches!(
                        self.checkpoint.state,
                        CollectionState::Queued
                            | CollectionState::Running
                            | CollectionState::Interrupted
                    ) && !self.checkpoint.cancellation_requested,
                    "Collection is not cancellable",
                )?;
                self.checkpoint.cancellation_requested = true;
                if matches!(
                    self.checkpoint.state,
                    CollectionState::Queued | CollectionState::Interrupted
                ) {
                    self.finish(CollectionState::Cancelled);
                }
            }
            CollectionEvent::StopAcknowledged { .. } => {
                self.running()?;
                require(
                    self.checkpoint.cancellation_requested,
                    "Cancellation was not requested",
                )?;
                // The caller must establish transport quiescence. No active transport exists in this slice.
                self.interrupt_reservation(at);
                self.finish(CollectionState::Cancelled);
            }
            CollectionEvent::Recover { .. } => {
                self.running()?;
                self.interrupt_reservation(at);
                self.checkpoint.state = if self.checkpoint.cancellation_requested {
                    CollectionState::Cancelled
                } else {
                    CollectionState::Interrupted
                };
                self.checkpoint.lease = None;
            }
        }
        self.checkpoint.updated_at_ms = at;
        Ok(promotion)
    }
    fn running(&self) -> Result<()> {
        require(
            self.checkpoint.state == CollectionState::Running && self.checkpoint.lease.is_some(),
            "Collection is not running",
        )
    }
    fn expired(&self, at: i64) -> bool {
        self.checkpoint
            .deadline_at_ms
            .is_some_and(|deadline| at >= deadline)
    }
    fn finish(&mut self, state: CollectionState) {
        self.checkpoint.state = state;
        self.checkpoint.lease = None;
    }
    fn interrupt_reservation(&mut self, at: i64) {
        for request in &mut self.checkpoint.requests {
            if matches!(request.progress, RequestProgress::Reserved) {
                request.progress = RequestProgress::InterruptedUnknown {
                    recovered_at_ms: at,
                };
                self.checkpoint.saw_unknown = true;
                if request.entry.purpose == Purpose::Robots {
                    let host = Url::parse(&request.entry.url)
                        .expect("validated URL")
                        .host_str()
                        .unwrap()
                        .to_owned();
                    self.policies.insert(host.clone(), None);
                    self.checkpoint.robots.push(RobotsCheckpoint {
                        host,
                        request_sequence: request.sequence,
                        usable: false,
                    });
                    self.checkpoint.saw_blocked = true;
                }
            }
        }
    }
    fn advance(&mut self, at: i64) -> Result<()> {
        self.running()?;
        require(
            !self
                .checkpoint
                .requests
                .iter()
                .any(|r| matches!(r.progress, RequestProgress::Reserved)),
            "A charged request is still unresolved",
        )?;
        if self.checkpoint.cancellation_requested {
            self.finish(CollectionState::Cancelled);
            return Ok(());
        }
        if self.expired(at) || self.checkpoint.saw_quota {
            self.finish(CollectionState::QuotaExhausted);
            return Ok(());
        }
        while let Some(entry) = self.checkpoint.frontier.first().cloned() {
            if self.checkpoint.visited.len() >= MAX_FRONTIER
                || self.checkpoint.requests_used() >= self.input.max_requests
            {
                self.finish(CollectionState::QuotaExhausted);
                return Ok(());
            }
            if self.checkpoint.visited.contains(&entry.url) {
                self.checkpoint.frontier.remove(0);
                continue;
            }
            let url = policy::validate_https_url(&entry.url)?;
            let host = url.host_str().unwrap().to_owned();
            require(
                self.hosts.contains(&host) && entry.hop <= self.input.max_hops,
                "Frontier is outside selected scope",
            )?;
            if !self.policies.contains_key(&host) {
                let mut robots = url;
                robots.set_path("/robots.txt");
                robots.set_query(None);
                self.reserve(
                    FrontierEntry {
                        url: robots.to_string(),
                        hop: 0,
                        redirects: 0,
                        purpose: Purpose::Robots,
                        parent: None,
                    },
                    at,
                )?;
                return Ok(());
            }
            self.checkpoint.frontier.remove(0);
            self.checkpoint.visited.push(entry.url.clone());
            let allowed = self
                .policies
                .get(&host)
                .and_then(Option::as_ref)
                .is_some_and(|rules| {
                    collection::robots_allowed(rules, &entry.url)
                        && !collection::robots_has_crawl_delay(rules)
                });
            if !allowed {
                self.checkpoint.saw_blocked = true;
                continue;
            }
            self.reserve(entry, at)?;
            return Ok(());
        }
        let state = if self.checkpoint.pages_retained > 0 {
            if self.checkpoint.saw_blocked
                || self.checkpoint.saw_failed
                || self.checkpoint.saw_unknown
            {
                CollectionState::Partial
            } else {
                CollectionState::Successful
            }
        } else if self.checkpoint.saw_unknown || self.checkpoint.saw_failed {
            CollectionState::Failed
        } else if self.checkpoint.saw_blocked {
            CollectionState::Blocked
        } else {
            CollectionState::SuccessfulNoResults
        };
        self.finish(state);
        Ok(())
    }
    fn reserve(&mut self, entry: FrontierEntry, at: i64) -> Result<()> {
        let mut budget = policy::DiscoveryBudget {
            hops: self.input.max_hops,
            requests: self.input.max_requests,
            seconds: self.input.max_seconds,
            used: self.checkpoint.requests_used(),
        };
        reserve_budget(
            &mut budget,
            self.checkpoint
                .first_started_at_ms
                .expect("running has start"),
            at,
            entry.hop,
        )?;
        self.checkpoint.requests.push(ChargedRequest {
            sequence: budget.used - 1,
            generation: self.checkpoint.generation,
            lease: self.checkpoint.lease.clone().expect("running has lease"),
            entry,
            reserved_at_ms: at,
            progress: RequestProgress::Reserved,
        });
        Ok(())
    }
    fn received(
        &mut self,
        request: &ChargedRequest,
        result: &FetchRecord,
        body: Option<&[u8]>,
        at: i64,
    ) -> Result<Option<Promotion>> {
        if self.expired(at) {
            self.checkpoint.saw_quota = true;
        }
        let entry = &request.entry;
        let url = policy::validate_https_url(&entry.url)?;
        if entry.purpose == Purpose::Robots {
            let rules = robots_rules(result, body);
            let host = url.host_str().unwrap().to_owned();
            self.checkpoint.robots.push(RobotsCheckpoint {
                host: host.clone(),
                request_sequence: request.sequence,
                usable: rules.is_some(),
            });
            if rules.is_none() {
                match result {
                    FetchRecord::Complete { status: 429, .. }
                    | FetchRecord::Failed {
                        reason: TransportFailure::Quota,
                    } => self.checkpoint.saw_quota = true,
                    FetchRecord::Complete { status, .. } if *status < 500 => {
                        self.checkpoint.saw_blocked = true
                    }
                    FetchRecord::Failed {
                        reason: TransportFailure::Policy,
                    } => self.checkpoint.saw_blocked = true,
                    _ => self.checkpoint.saw_failed = true,
                }
            }
            self.policies.insert(host, rules);
            return Ok(None);
        }
        match result {
            FetchRecord::Complete {
                status,
                redirect_url,
                media_type,
                sha256,
                ..
            } => {
                if *status == 429 {
                    self.checkpoint.saw_quota = true;
                }
                if self.checkpoint.cancellation_requested || self.checkpoint.saw_quota {
                    return Ok(None);
                }
                if [301, 302, 303, 307, 308].contains(status) {
                    if entry.redirects >= 5 {
                        self.checkpoint.saw_blocked = true;
                    } else if let Some(next) = redirect_url {
                        let next = policy::validate_https_url(next)?;
                        if self
                            .hosts
                            .iter()
                            .any(|h| Some(h.as_str()) == next.host_str())
                        {
                            self.enqueue(
                                FrontierEntry {
                                    url: next.to_string(),
                                    hop: entry.hop,
                                    redirects: entry.redirects + 1,
                                    purpose: Purpose::Redirect,
                                    parent: Some(request.sequence),
                                },
                                true,
                            );
                        } else {
                            self.checkpoint.saw_blocked = true;
                        }
                    } else {
                        self.checkpoint.saw_failed = true;
                    }
                } else if *status == 200 {
                    let interpreted = collection::static_page(
                        body.expect("complete has body"),
                        media_type.as_deref().unwrap_or(""),
                        &url,
                    );
                    let interpreted = match interpreted {
                        Ok(value) => value,
                        Err(Error::QuotaExhausted(_)) if self.version >= 4 => {
                            // The complete receipt/original still publishes. No
                            // prefix text, links or successful page is accepted.
                            self.checkpoint.saw_quota = true;
                            return Ok(None);
                        }
                        Err(Error::QuotaExhausted(reason)) => {
                            return Err(Error::Blocked(format!(
                                "Historical HTML interpretation refused without rewriting its journal: {reason}"
                            )));
                        }
                        Err(error) => return Err(error),
                    };
                    if let Some((text, links)) = interpreted {
                        if entry.hop < self.input.max_hops {
                            for link in links {
                                if self
                                    .hosts
                                    .iter()
                                    .any(|h| Some(h.as_str()) == link.host_str())
                                {
                                    self.enqueue(
                                        FrontierEntry {
                                            url: link.to_string(),
                                            hop: entry.hop + 1,
                                            redirects: 0,
                                            purpose: Purpose::Link,
                                            parent: Some(request.sequence),
                                        },
                                        false,
                                    );
                                }
                            }
                        }
                        self.checkpoint.pages_retained += 1;
                        return Ok(Some(Promotion {
                            sha256: sha256.clone(),
                            text,
                            media_type: media_type.clone().unwrap_or_default(),
                        }));
                    } else {
                        self.checkpoint.saw_failed = true;
                    }
                } else if ![204, 205].contains(status) {
                    self.checkpoint.saw_failed = true;
                }
            }
            _ => self.record_failure(result),
        }
        Ok(None)
    }
    fn enqueue(&mut self, entry: FrontierEntry, first: bool) {
        if self.checkpoint.frontier.len() < MAX_FRONTIER {
            if first {
                self.checkpoint.frontier.insert(0, entry);
            } else {
                self.checkpoint.frontier.push(entry);
            }
        } else {
            self.checkpoint.saw_quota = true;
        }
    }
    fn record_failure(&mut self, result: &FetchRecord) {
        match result {
            FetchRecord::Complete { status: 429, .. }
            | FetchRecord::Failed {
                reason: TransportFailure::Quota,
            } => self.checkpoint.saw_quota = true,
            FetchRecord::Failed {
                reason: TransportFailure::Policy,
            } => self.checkpoint.saw_blocked = true,
            _ => self.checkpoint.saw_failed = true,
        }
    }
}

pub(crate) fn valid_time(at: i64) -> Result<()> {
    require(
        at >= 0 && at <= chrono::Utc::now().timestamp_millis() + 5000,
        "Invalid or future collection timestamp",
    )
}
pub(crate) fn validate_fetch(result: &FetchRecord, body: Option<&[u8]>) -> Result<()> {
    match result {
        FetchRecord::Complete {
            status,
            media_type,
            redirect_url,
            sha256,
            bytes,
        } => {
            require(
                (100..=599).contains(status) && *bytes <= PAGE_BYTES as u64,
                "Invalid complete response metadata",
            )?;
            valid_media(media_type)?;
            require(
                body.is_some_and(|body| {
                    body.len() as u64 == *bytes && crate::store::hash(body) == *sha256
                }),
                "Response original binding failed",
            )?;
            if let Some(target) = redirect_url {
                require(
                    [301, 302, 303, 307, 308].contains(status)
                        && policy::validate_https_url(target)?.as_str() == target,
                    "Invalid safe redirect metadata",
                )?;
            }
        }
        FetchRecord::Incomplete { status, media_type } => {
            require(
                (100..=599).contains(status) && body.is_none(),
                "Incomplete response cannot have original bytes",
            )?;
            valid_media(media_type)?;
        }
        FetchRecord::Failed { .. } => {
            require(body.is_none(), "Failed request cannot have original bytes")?
        }
    }
    Ok(())
}
fn valid_media(media: &Option<String>) -> Result<()> {
    require(
        media
            .as_ref()
            .is_none_or(|m| collection::media_type(m).as_ref() == Some(m)),
        "Invalid normalized response media type",
    )
}

#[cfg(test)]
thread_local! {
    static REPLAY_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn replay_calls() -> usize {
    REPLAY_CALLS.get()
}

pub(crate) fn replay(
    job: &DurableCollectionJob,
    mut original: impl FnMut(&ChargedRequest, &FetchRecord) -> Result<Option<Vec<u8>>>,
) -> Result<Machine> {
    #[cfg(test)]
    REPLAY_CALLS.set(REPLAY_CALLS.get() + 1);
    job.protocol()?;
    require(
        canonical_uuid(&job.id)
            && canonical_uuid(&job.request_key)
            && job.events.len() <= MAX_EVENTS,
        "Unsupported or malformed durable collection record",
    )?;
    let mut machine =
        Machine::new_version(&job.input, job.created_at_ms, job.schema_version, true)?;
    for event in &job.events {
        // The v2 receipt's raw wall sample may be backwards. Its clock anchor
        // identifies the prior valid journal state, not an invented completion time.
        if job.schema_version == 1 {
            valid_time(event.at_ms())?;
        }
        require(
            event.at_ms() >= machine.checkpoint.updated_at_ms,
            "Collection clock moved backwards",
        )?;
        let completion = match event {
            CollectionEvent::Complete {
                sequence, result, ..
            } => Some((*sequence, result.clone())),
            CollectionEvent::TransportObserved {
                sequence, receipt, ..
            } => {
                require(
                    matches!(job.schema_version, 2..=4),
                    "V1 cannot contain transport receipts",
                )?;
                require(
                    chrono::DateTime::from_timestamp_millis(receipt.observed_wall_ms).is_some(),
                    "Transport clock sample is not representable",
                )?;
                receipt.validate_mode(job.synthetic)?;
                Some((*sequence, receipt.fetch_record()))
            }
            _ => None,
        };
        let body = if let Some((sequence, result)) = completion {
            let request = machine
                .checkpoint
                .requests
                .get(sequence as usize)
                .ok_or_else(|| Error::Validation("Completion has no charged reservation".into()))?;
            original(request, &result)?
        } else {
            None
        };
        machine.apply(event, body.as_deref())?;
    }
    require(
        machine.checkpoint == job.checkpoint,
        "Stored collection checkpoint differs from verified event replay",
    )?;
    machine.replaying = false;
    Ok(machine)
}

#[path = "collection_machine_transport.rs"]
mod transport;

/// Shared arithmetic only; callers validate new/replayed times and input ceilings.
pub(crate) fn first_deadline(at: i64, seconds: u64) -> Result<i64> {
    at.checked_add(seconds as i64 * 1000)
        .ok_or_else(|| Error::Validation("Invalid collection deadline".into()))
}
pub(crate) fn reserve_budget(
    budget: &mut policy::DiscoveryBudget,
    first_started: i64,
    at: i64,
    hop: u32,
) -> Result<()> {
    let elapsed = (at - first_started) as u64 / 1000;
    budget.reserve(hop, elapsed, u32::MAX)
}
pub(crate) fn robots_rules(result: &FetchRecord, body: Option<&[u8]>) -> Option<String> {
    match result {
        FetchRecord::Complete { status: 404, .. } => Some(String::new()),
        FetchRecord::Complete { status: 200, .. } => body
            .filter(|b| b.len() <= 512_000)
            .and_then(|b| std::str::from_utf8(b).ok())
            .map(str::to_owned),
        _ => None,
    }
}

/// Keep transport stop precedence identical across private interpreters.
pub(crate) fn receipt_stop_state(
    receipt: &crate::collection_settlement::TransportReceipt,
    cancelled: bool,
    deadline: i64,
    reserved_at: i64,
    max_seconds: u64,
) -> Option<CollectionState> {
    use crate::collection_transport::StopReason;
    if !receipt.locally_quiescent {
        Some(CollectionState::RecoveryRequired)
    } else if receipt.stopped_for(StopReason::ClockChanged)
        || receipt.observed_wall_ms < reserved_at
    {
        Some(CollectionState::Failed)
    } else if cancelled || receipt.stopped_for(StopReason::Cancelled) {
        Some(CollectionState::Cancelled)
    } else if receipt.observed_wall_ms >= deadline
        || receipt.stopped_for(StopReason::Deadline)
        || receipt.stopped_for(StopReason::BodyLimit)
        || receipt.elapsed_milliseconds >= max_seconds * 1000
    {
        Some(CollectionState::QuotaExhausted)
    } else if receipt.stopped_for(StopReason::Busy) {
        Some(CollectionState::Blocked)
    } else {
        None
    }
}
