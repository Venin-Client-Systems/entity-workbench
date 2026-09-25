//! Access prefix only: no link expansion, redirect continuation or transport.
#![allow(dead_code)]
use crate::{
    collection,
    collection_access::*,
    collection_jobs::{CollectionState, FetchRecord, RequestProgress, MAX_EVENTS},
    collection_machine::{self, Promotion},
    collection_profile::ValidatedPublisherAccessPlan,
    collection_settlement::TransportReceipt,
    policy, require, Error, Result,
};
use std::collections::BTreeMap;

pub(crate) struct Machine {
    pub checkpoint: Checkpoint,
    profile: ValidatedPublisherAccessPlan,
    robots: BTreeMap<String, Option<String>>,
}
impl Machine {
    pub(crate) fn new(
        profile: ValidatedPublisherAccessPlan,
        at: i64,
        replay: bool,
    ) -> Result<Self> {
        if replay {
            require(at >= 0, "Invalid access timestamp")?;
        } else {
            collection_machine::valid_time(at)?;
        }
        Ok(Self {
            profile,
            robots: BTreeMap::new(),
            checkpoint: Checkpoint {
                state: State::Queued,
                phase: Phase::Access,
                generation: 1,
                lease: None,
                first_started_at_ms: None,
                deadline_at_ms: None,
                updated_at_ms: at,
                cancellation_requested: false,
                access_cursor: 0,
                robots: Vec::new(),
                requests: Vec::new(),
                decision: None,
                content_request: None,
                promoted: false,
            },
        })
    }
    fn finish(&mut self, state: State) {
        self.checkpoint.state = state;
        self.checkpoint.lease = None;
    }
    fn expired(&self, at: i64) -> bool {
        self.checkpoint
            .deadline_at_ms
            .is_some_and(|deadline| at >= deadline)
    }
    fn running(&self) -> Result<()> {
        require(
            self.checkpoint.state == State::Running && self.checkpoint.lease.is_some(),
            "Access run is not executing",
        )
    }
    fn apply(
        &mut self,
        event: &Event,
        body: Option<&[u8]>,
        historical: bool,
    ) -> Result<Option<Promotion>> {
        if let Event::Observed {
            clock_anchor_ms,
            sequence,
            receipt,
        } = event
        {
            return self.observe(*clock_anchor_ms, *sequence, receipt, body);
        }
        let at = event.at_ms();
        if historical {
            require(at >= 0, "Invalid access timestamp")?;
        } else {
            collection_machine::valid_time(at)?;
        }
        require(
            at >= self.checkpoint.updated_at_ms,
            "Access clock moved backwards",
        )?;
        match event {
            Event::Observed { .. } => unreachable!(),
            Event::Start { lease, .. } => {
                require(
                    self.checkpoint.state == State::Queued
                        && crate::collection_jobs::canonical_uuid(lease),
                    "Invalid access start",
                )?;
                self.checkpoint.first_started_at_ms = Some(at);
                self.checkpoint.deadline_at_ms = Some(collection_machine::first_deadline(
                    at,
                    self.profile.input().effective_limits.max_seconds,
                )?);
                self.checkpoint.lease = Some(lease.clone());
                self.checkpoint.state = State::Running;
            }
            Event::Resume { lease, .. } => {
                require(
                    matches!(
                        self.checkpoint.state,
                        State::Interrupted | State::ReadyContent
                    ) && !self.checkpoint.cancellation_requested
                        && self.checkpoint.generation < 8
                        && crate::collection_jobs::canonical_uuid(lease)
                        && self.checkpoint.requests.iter().all(|r| {
                            r.lease != *lease
                                && !matches!(
                                    r.progress,
                                    RequestProgress::Reserved
                                        | RequestProgress::InterruptedUnknown { .. }
                                )
                        }),
                    "Access cannot resume or lease was reused",
                )?;
                self.checkpoint.generation += 1;
                self.checkpoint.lease = Some(lease.clone());
                self.checkpoint.state = State::Running;
                if self.expired(at) {
                    self.finish(State::QuotaExhausted);
                }
            }
            Event::Advance { .. } => self.advance(at)?,
            Event::Decide { decision, .. } => {
                decision.validate()?;
                require(
                    self.checkpoint.state == State::AwaitingDecision
                        && self.checkpoint.decision.is_none(),
                    "Access is not awaiting a decision",
                )?;
                self.checkpoint.decision = Some(decision.clone());
                if self.expired(at) {
                    self.finish(State::QuotaExhausted);
                } else if decision.outcome == DecisionOutcome::Allow {
                    self.checkpoint.phase = Phase::Content;
                    self.finish(State::ReadyContent);
                } else {
                    self.finish(State::Blocked);
                }
            }
            Event::Cancel { .. } => {
                require(
                    matches!(
                        self.checkpoint.state,
                        State::Queued
                            | State::Running
                            | State::AwaitingDecision
                            | State::ReadyContent
                            | State::Interrupted
                    ) && !self.checkpoint.cancellation_requested,
                    "Access is not cancellable",
                )?;
                self.checkpoint.cancellation_requested = true;
                if self.checkpoint.state != State::Running {
                    self.finish(State::Cancelled);
                }
            }
            Event::Recover { .. } => {
                self.running()?;
                for request in &mut self.checkpoint.requests {
                    if request.progress == RequestProgress::Reserved {
                        request.progress = RequestProgress::InterruptedUnknown {
                            recovered_at_ms: at,
                        };
                    }
                }
                self.finish(if self.checkpoint.cancellation_requested {
                    State::Cancelled
                } else {
                    State::Interrupted
                });
            }
        }
        self.checkpoint.updated_at_ms = at;
        Ok(None)
    }
    fn robots_url(&self, host: &str) -> Result<String> {
        let url = format!("https://{host}/robots.txt");
        self.profile.validate_destination_syntax(&url)?;
        Ok(url)
    }
    fn allowed(&self, host: &str, url: &str) -> bool {
        self.robots
            .get(host)
            .and_then(Option::as_ref)
            .is_some_and(|rules| {
                collection::robots_allowed(rules, url) && !collection::robots_has_crawl_delay(rules)
            })
    }
    fn advance(&mut self, at: i64) -> Result<()> {
        self.running()?;
        require(
            self.checkpoint
                .requests
                .iter()
                .all(|r| r.progress != RequestProgress::Reserved),
            "Access request is unresolved",
        )?;
        if self.checkpoint.cancellation_requested {
            self.finish(State::Cancelled);
            return Ok(());
        }
        if self.expired(at) {
            self.finish(State::QuotaExhausted);
            return Ok(());
        }
        let seed = self.profile.input().seed_url.clone();
        let seed_host = self
            .profile
            .validate_destination_syntax(&seed)?
            .host()
            .to_owned();
        if self.checkpoint.phase == Phase::Access {
            if !self.robots.contains_key(&seed_host) {
                return self.reserve(
                    self.robots_url(&seed_host)?,
                    Purpose::Robots { host: seed_host },
                    at,
                );
            }
            if !self.allowed(&seed_host, &seed) {
                self.finish(State::Blocked);
                return Ok(());
            }
            while let Some(raw) = self
                .profile
                .input()
                .access_urls
                .get(self.checkpoint.access_cursor as usize)
                .cloned()
            {
                let host = self.profile.selected_access_url(&raw)?.host().to_owned();
                if !self.robots.contains_key(&host) {
                    return self.reserve(self.robots_url(&host)?, Purpose::Robots { host }, at);
                }
                // A selected robots URL binds the earlier exact acquisition. No second charge.
                if raw == self.robots_url(&host)? {
                    let selected_document = self
                        .checkpoint
                        .robots
                        .iter()
                        .find(|r| r.host == host)
                        .and_then(|r| self.checkpoint.requests.get(r.sequence as usize))
                        .is_some_and(|r| match &r.progress {
                            RequestProgress::Observed { receipt } => {
                                matches!(
                                    receipt.fetch_record(),
                                    FetchRecord::Complete { status: 200, .. }
                                ) && receipt.head().is_some_and(|h| h.identity_encoding)
                            }
                            _ => false,
                        });
                    // A derived robots 404 can permit a path, but is not a fetched
                    // document satisfying an explicit selected-access prerequisite.
                    if !selected_document {
                        self.finish(State::Blocked);
                        return Ok(());
                    }
                    self.checkpoint.access_cursor += 1;
                    continue;
                }
                if !self.allowed(&host, &raw) {
                    self.finish(State::Blocked);
                    return Ok(());
                }
                return self.reserve(
                    raw,
                    Purpose::SelectedAccess {
                        index: self.checkpoint.access_cursor,
                    },
                    at,
                );
            }
            self.finish(State::AwaitingDecision);
            return Ok(());
        }
        require(
            self.checkpoint
                .decision
                .as_ref()
                .is_some_and(|d| d.outcome == DecisionOutcome::Allow)
                && self.checkpoint.content_request.is_none(),
            "Content prefix is not authorized or already complete",
        )?;
        if !self.allowed(&seed_host, &seed) {
            self.finish(State::Blocked);
            return Ok(());
        }
        self.reserve(seed, Purpose::Seed {}, at)
    }
    fn reserve(&mut self, url: String, purpose: Purpose, at: i64) -> Result<()> {
        let limits = self.profile.input().effective_limits;
        if self.checkpoint.requests.len() >= limits.max_requests as usize {
            self.finish(State::QuotaExhausted);
            return Ok(());
        }
        let mut budget = policy::DiscoveryBudget {
            hops: limits.max_hops,
            requests: limits.max_requests,
            seconds: limits.max_seconds,
            used: self.checkpoint.requests.len() as u32,
        };
        collection_machine::reserve_budget(
            &mut budget,
            self.checkpoint.first_started_at_ms.expect("started access"),
            at,
            0,
        )?;
        let host = self
            .profile
            .validate_destination_syntax(&url)?
            .host()
            .to_owned();
        let robots_sequence = if matches!(purpose, Purpose::Robots { .. }) {
            None
        } else {
            self.checkpoint
                .robots
                .iter()
                .find(|r| r.host == host && r.usable)
                .map(|r| r.sequence)
        };
        require(
            matches!(purpose, Purpose::Robots { .. }) || robots_sequence.is_some(),
            "Request lacks usable robots acquisition",
        )?;
        self.checkpoint.requests.push(Request {
            sequence: budget.used - 1,
            generation: self.checkpoint.generation,
            lease: self.checkpoint.lease.clone().expect("executing lease"),
            url,
            purpose,
            content_parent: None,
            robots_sequence,
            reserved_at_ms: at,
            progress: RequestProgress::Reserved,
        });
        Ok(())
    }
    fn observe(
        &mut self,
        anchor: i64,
        sequence: u32,
        receipt: &TransportReceipt,
        body: Option<&[u8]>,
    ) -> Result<Option<Promotion>> {
        self.running()?;
        require(
            anchor == self.checkpoint.updated_at_ms,
            "Access observation clock anchor changed",
        )?;
        let request = self
            .checkpoint
            .requests
            .get(sequence as usize)
            .ok_or_else(|| Error::Validation("No charged access request".into()))?
            .clone();
        require(
            request.sequence == sequence && request.progress == RequestProgress::Reserved,
            "Access request is not reserved",
        )?;
        let url = self.profile.validate_destination_syntax(&request.url)?;
        receipt.validate_mode(true)?;
        require(
            receipt
                .resolved
                .as_ref()
                .is_none_or(|r| r.method == "synthetic_fixed_candidates"),
            "Experimental prefix requires synthetic candidate observations",
        )?;
        receipt.validate_scoped(body, |addresses| {
            policy::validate_destination(url.as_str(), addresses, &[url.host()]).map(|_| ())
        })?;
        let clock_changed = receipt
            .stopped_for(crate::collection_transport::StopReason::ClockChanged)
            || receipt.observed_wall_ms < request.reserved_at_ms;
        let mut promotion = None;
        if let Some(state) = collection_machine::receipt_stop_state(
            receipt,
            self.checkpoint.cancellation_requested,
            self.checkpoint.deadline_at_ms.expect("started deadline"),
            request.reserved_at_ms,
            self.profile.input().effective_limits.max_seconds,
        ) {
            let next = match state {
                CollectionState::RecoveryRequired => State::RecoveryRequired,
                CollectionState::Failed => State::Failed,
                CollectionState::Cancelled => {
                    self.checkpoint.cancellation_requested = true;
                    State::Cancelled
                }
                CollectionState::QuotaExhausted => State::QuotaExhausted,
                CollectionState::Blocked => State::Blocked,
                _ => unreachable!("closed receipt stop classification"),
            };
            self.finish(next);
        } else if receipt.head().is_some_and(|h| !h.identity_encoding) {
            self.finish(State::Blocked);
        } else {
            let result = receipt.fetch_record();
            match &request.purpose {
                Purpose::Robots { host } => {
                    let rules = collection_machine::robots_rules(&result, body);
                    self.checkpoint.robots.push(Robots {
                        host: host.clone(),
                        sequence,
                        usable: rules.is_some(),
                    });
                    if rules.is_none() {
                        self.finish(failure_state(&result));
                    }
                    self.robots.insert(host.clone(), rules);
                }
                Purpose::SelectedAccess { index } => {
                    require(
                        *index == self.checkpoint.access_cursor,
                        "Selected access ordering changed",
                    )?;
                    if matches!(result, FetchRecord::Complete { status: 200, .. }) {
                        self.checkpoint.access_cursor += 1;
                    } else {
                        self.finish(failure_state(&result));
                    }
                }
                Purpose::Seed {} => {
                    self.checkpoint.content_request = Some(sequence);
                    if let FetchRecord::Complete {
                        status: 200,
                        sha256,
                        media_type,
                        ..
                    } = &result
                    {
                        let parsed = collection::static_page(
                            body.expect("complete original"),
                            media_type.as_deref().unwrap_or(""),
                            &url::Url::parse(&request.url).expect("validated URL"),
                        );
                        match parsed {
                            Ok(Some((text, _links))) => {
                                promotion = Some(Promotion {
                                    sha256: sha256.clone(),
                                    text,
                                    media_type: media_type.clone().unwrap_or_default(),
                                });
                                self.checkpoint.promoted = true;
                                self.finish(State::ContentBoundary);
                            }
                            Ok(None) => self.finish(State::Blocked),
                            Err(Error::QuotaExhausted(_)) => self.finish(State::QuotaExhausted),
                            Err(error) => return Err(error),
                        }
                    } else {
                        self.finish(failure_state(&result));
                    }
                }
            }
        }
        self.checkpoint.requests[sequence as usize].progress = RequestProgress::Observed {
            receipt: receipt.clone(),
        };
        if !clock_changed {
            self.checkpoint.updated_at_ms = anchor.max(receipt.observed_wall_ms);
        }
        Ok(promotion)
    }
}
fn failure_state(result: &FetchRecord) -> State {
    match result {
        FetchRecord::Complete { status: 429, .. } => State::QuotaExhausted,
        FetchRecord::Complete { status, .. } if *status < 500 => State::Blocked,
        _ => State::Failed,
    }
}
pub(crate) fn append(
    job: &mut Job,
    machine: &mut Machine,
    event: Event,
    body: Option<&[u8]>,
    historical: bool,
) -> Result<Option<Promotion>> {
    require(
        job.events.len() < MAX_EVENTS,
        "Experimental access event bound reached",
    )?;
    if let Event::Decide { decision, .. } = &event {
        require(
            review(job)?.review_sha256 == decision.review_sha256,
            "Access review binding changed",
        )?;
    }
    let promotion = machine.apply(&event, body, historical)?;
    job.events.push(event);
    job.checkpoint = machine.checkpoint.clone();
    job.bounded()?;
    Ok(promotion)
}
pub(crate) fn replay(
    job: &Job,
    mut original: impl FnMut(&Request, &TransportReceipt) -> Result<Option<Vec<u8>>>,
) -> Result<(Machine, Option<Promotion>)> {
    let profile = job.profile()?;
    let mut machine = Machine::new(profile, job.created_at_ms, true)?;
    let mut prefix = job.clone();
    prefix.events.clear();
    prefix.checkpoint = machine.checkpoint.clone();
    let mut promoted = None;
    for event in &job.events {
        let body = if let Event::Observed {
            sequence, receipt, ..
        } = event
        {
            let request = machine
                .checkpoint
                .requests
                .get(*sequence as usize)
                .ok_or_else(|| Error::Validation("Observation has no reservation".into()))?;
            original(request, receipt)?
        } else {
            None
        };
        if let Some(promotion) = append(
            &mut prefix,
            &mut machine,
            event.clone(),
            body.as_deref(),
            true,
        )? {
            promoted = Some(promotion);
        }
    }
    require(
        prefix == *job,
        "Experimental access checkpoint differs from replay",
    )?;
    Ok((machine, promoted))
}
