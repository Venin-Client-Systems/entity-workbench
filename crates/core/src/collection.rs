//! Shared static collection rules and historical synthetic receipt tests.
//! Legacy synchronous live collection is disabled during the durable cutover.
#[cfg(test)]
use crate::collection_receipt::{FetchOutcome, RequestPurpose};
use crate::{
    collection_receipt::{AcquisitionMode, RequestReceipt},
    policy, require, Error, Result,
};
use scraper::{Html, Selector};
use serde::Serialize;
#[cfg(test)]
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    time::Instant,
};
use url::Url;
pub(crate) const PAGE_BYTES: usize = 2 * 1024 * 1024;
const AGENT: &str = "EntityWorkbench";
#[derive(Debug)]
pub struct Page {
    pub request_sequence: u32,
    pub url: String,
    pub bytes: Vec<u8>,
    pub text: String,
}
#[derive(Debug, Serialize)]
pub struct CollectionResult {
    #[serde(skip)]
    pub pages: Vec<Page>,
    pub requests: u32,
    pub state: crate::domain::JobState,
    pub notes: Vec<String>,
    pub mode: AcquisitionMode,
    pub started_at: String,
    pub ended_at: String,
    pub elapsed_milliseconds: u64,
    pub trace: Vec<RequestReceipt>,
    #[serde(skip)]
    pub responses: Vec<ResponseBody>,
}
#[derive(Debug)]
pub struct ResponseBody {
    pub sequence: u32,
    pub bytes: Vec<u8>,
}
pub fn validate_seeds(seeds: &[String]) -> Result<Vec<Url>> {
    require(
        !seeds.is_empty() && seeds.len() <= 10,
        "Select one to ten seed URLs",
    )?;
    seeds
        .iter()
        .map(|raw| policy::validate_https_url(raw))
        .collect()
}
#[cfg(test)]
struct Fetched {
    status: u16,
    body: Vec<u8>,
    redirect: Option<String>,
    content_type: String,
    complete: bool,
}
#[cfg(test)]
trait Transport {
    fn mode(&self) -> AcquisitionMode {
        AcquisitionMode::Synthetic
    }
    fn fetch(&mut self, url: &Url, hop: u32) -> Result<Fetched>;
    fn requests_used(&self) -> u32;
    fn elapsed_seconds(&self) -> u64;
}
// Only parsed media type and a policy-validated redirect target are retained;
// cookies, authentication headers and unsafe Location values never enter receipts.
pub(crate) fn media_type(raw: &str) -> Option<String> {
    let value = raw.split(';').next()?.trim().to_ascii_lowercase();
    (!value.is_empty()
        && value.len() <= 128
        && value.is_ascii()
        && !value.chars().any(char::is_control))
    .then_some(value)
}

#[cfg(test)]
struct Trace {
    started_at: String,
    wall: chrono::DateTime<chrono::Utc>,
    monotonic: Instant,
    requests: Vec<RequestReceipt>,
    responses: Vec<ResponseBody>,
}
#[cfg(test)]
impl Trace {
    fn new() -> Self {
        let wall = chrono::Utc::now();
        Self {
            started_at: wall.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            wall,
            monotonic: Instant::now(),
            requests: vec![],
            responses: vec![],
        }
    }
    fn stamp(&self) -> String {
        (self.wall + chrono::Duration::milliseconds(self.monotonic.elapsed().as_millis() as i64))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
    fn fetch(
        &mut self,
        broker: &mut impl Transport,
        url: &Url,
        hop: u32,
        purpose: RequestPurpose,
        parent: Option<u32>,
    ) -> (Option<u32>, Result<Fetched>) {
        let before = broker.requests_used();
        let started_at = self.stamp();
        let mut result = broker.fetch(url, hop);
        let used = broker.requests_used();
        if used == before {
            // A refused reservation is a stop decision, not a charged fetch attempt.
            if result.is_ok() {
                result = Err(Error::Validation(
                    "Transport returned an uncharged response".into(),
                ));
            }
            return (None, result);
        }
        if used != before + 1 || self.requests.len() != before as usize {
            return (
                None,
                Err(Error::Validation(
                    "Transport request accounting is inconsistent".into(),
                )),
            );
        }
        let sequence = self.requests.len() as u32;
        let mut request = RequestReceipt {
            sequence,
            url: url.to_string(),
            method: "GET".into(),
            purpose,
            parent_request: parent,
            hop,
            started_at,
            ended_at: self.stamp(),
            outcome: FetchOutcome::Failed,
            http_status: None,
            media_type: None,
            redirect_url: None,
            body_sha256: None,
            body_bytes: None,
            original_evidence_id: None,
        };
        match &mut result {
            Ok(response) => {
                if response.body.len() > PAGE_BYTES {
                    response.complete = false;
                    response.body.clear();
                }
                request.http_status = Some(response.status);
                request.media_type = media_type(&response.content_type);
                if [301, 302, 303, 307, 308].contains(&response.status) {
                    request.redirect_url = response
                        .redirect
                        .as_ref()
                        .and_then(|raw| url.join(raw).ok())
                        .and_then(|u| policy::validate_https_url(u.as_str()).ok())
                        .map(|u| u.to_string());
                }
                if response.complete {
                    request.outcome = FetchOutcome::Fetched;
                    request.body_sha256 = Some(crate::store::hash(&response.body));
                    request.body_bytes = Some(response.body.len() as u64);
                    self.responses.push(ResponseBody {
                        sequence,
                        bytes: response.body.clone(),
                    });
                } else {
                    request.outcome = FetchOutcome::Incomplete;
                }
            }
            Err(Error::Blocked(_) | Error::Validation(_) | Error::QuotaExhausted(_)) => {
                request.outcome = FetchOutcome::Blocked;
            }
            Err(_) => {}
        }
        self.requests.push(request);
        (Some(sequence), result)
    }
}

/// Shared interpretation only. A complete response remains evidence even when no
/// searchable derivative or usable robots policy can be produced.
pub(crate) fn static_page(raw: &[u8], content_type: &str, url: &Url) -> Option<(String, Vec<Url>)> {
    let text = std::str::from_utf8(raw).ok()?;
    match media_type(content_type).as_deref() {
        Some("text/html") => Some(extract_html(text, url)),
        Some("text/plain") => Some((text.to_owned(), Vec::new())),
        _ => None,
    }
}

pub(crate) fn robots_allowed(policy: &str, url: &str) -> bool {
    robotstxt::DefaultMatcher::default().one_agent_allowed_by_robots(policy, AGENT, url)
}

pub(crate) fn robots_has_crawl_delay(policy: &str) -> bool {
    policy
        .lines()
        .any(|line| line.trim().to_ascii_lowercase().starts_with("crawl-delay:"))
}

/// Memory-safe static text/link extraction. Never executes or renders source markup.
pub fn extract_html(raw: &str, base: &Url) -> (String, Vec<Url>) {
    let html = Html::parse_document(raw);
    let text = html
        .root_element()
        .descendants()
        .filter_map(|node| {
            let value = node.value().as_text()?;
            if node.ancestors().any(|p| {
                p.value().as_element().is_some_and(|e| {
                    ["script", "style", "svg", "noscript", "template"].contains(&e.name())
                })
            }) {
                None
            } else {
                Some(value.text.as_ref())
            }
        })
        .collect::<Vec<&str>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let selector = Selector::parse("a[href]").expect("constant selector");
    let links = html
        .select(&selector)
        .take(1000)
        .filter_map(|a| base.join(a.value().attr("href")?).ok())
        .filter_map(|u| policy::validate_https_url(u.as_str()).ok())
        .collect();
    (text, links)
}
pub fn collect(
    seeds: Vec<String>,
    hops: u32,
    requests: u32,
    seconds: u64,
) -> Result<CollectionResult> {
    crate::collection_api::preview(crate::collection_api::CollectionInput {
        urls: seeds,
        max_hops: hops,
        max_requests: requests,
        max_seconds: seconds,
    })?;
    Err(Error::Blocked(
        "Synchronous collection is disabled; durable execution requires an enabled coordinator"
            .into(),
    ))
}

// Historical synthetic receipt tests only; no alternate production network path.
#[cfg(test)]
fn collect_with_transport(
    seeds: Vec<Url>,
    hops: u32,
    requests: u32,
    seconds: u64,
    mut broker: impl Transport,
) -> Result<CollectionResult> {
    let hosts: Vec<_> = seeds
        .iter()
        .filter_map(|s| s.host_str().map(str::to_owned))
        .collect();
    let mut queue: VecDeque<_> = seeds
        .into_iter()
        .map(|s| (s, 0, 0, RequestPurpose::Seed, None))
        .collect();
    let mode = broker.mode();
    let mut trace = Trace::new();
    let mut visited = HashSet::new();
    let mut robots: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut pages = vec![];
    let mut notes = vec![];
    let mut outcome = Outcome::default();
    while let Some((url, hop, redirects, purpose, parent)) = queue.pop_front() {
        if visited.len() >= 500 {
            outcome.exhausted = true;
            break;
        }
        if broker.requests_used() >= requests || broker.elapsed_seconds() >= seconds {
            outcome.exhausted = true;
            break;
        }
        if !visited.insert(url.to_string()) {
            continue;
        }
        let host = url.host_str().unwrap_or_default().to_owned();
        if !hosts.contains(&host) {
            continue;
        }
        if !robots.contains_key(&host) {
            let mut robots_url = url.clone();
            robots_url.set_path("/robots.txt");
            robots_url.set_query(None);
            let (_, fetched) = trace.fetch(
                &mut broker,
                &robots_url,
                0,
                RequestPurpose::AccessReview,
                None,
            );
            match fetched {
                Ok(r) if !r.complete => {
                    outcome.failed = true;
                    notes.push(format!(
                        "Incomplete robots response from {host}; no original accepted"
                    ));
                    robots.insert(host.clone(), None);
                }
                Ok(r) if r.status == 404 => {
                    robots.insert(host.clone(), Some(String::new()));
                }
                Ok(r) if r.status == 200 && r.body.len() <= 512_000 => {
                    match String::from_utf8(r.body) {
                        Ok(policy) => {
                            robots.insert(host.clone(), Some(policy));
                        }
                        Err(_) => {
                            outcome.blocked = true;
                            notes.push(format!(
                                "Robots policy encoding unsupported for {host}; host skipped"
                            ));
                            robots.insert(host.clone(), None);
                        }
                    }
                }
                Ok(r) if r.status == 429 => {
                    outcome.exhausted = true;
                    notes.push(format!(
                        "Website rate limit reached for {host} robots policy; no retries"
                    ));
                    break;
                }
                Ok(r) => {
                    if r.status >= 500 {
                        outcome.failed = true;
                    } else {
                        outcome.blocked = true;
                    }
                    notes.push(format!(
                        "Robots policy unavailable for {host} (HTTP {}); host skipped",
                        r.status
                    ));
                    robots.insert(host.clone(), None);
                }
                Err(error) => {
                    outcome.record_error(&error);
                    notes.push(format!(
                        "Robots policy unavailable for {host}; host skipped. {error}"
                    ));
                    robots.insert(host.clone(), None);
                    if outcome.exhausted {
                        break;
                    }
                }
            }
        }
        let Some(policy) = robots.get(&host).and_then(Option::as_ref) else {
            continue;
        };
        if !robots_allowed(policy, url.as_str()) {
            outcome.blocked = true;
            notes.push(format!("Robots policy denied {}", url.path()));
            continue;
        }
        // A crawl-delay extension is treated conservatively until per-host scheduling
        // is implemented. No directive is silently ignored to increase throughput.
        if robots_has_crawl_delay(policy) {
            outcome.blocked = true;
            notes.push(format!(
                "Crawl-delay policy requires manual scheduling for {host}"
            ));
            continue;
        }
        if broker.requests_used() >= requests {
            outcome.exhausted = true;
            break;
        }
        let (request_sequence, fetched) = trace.fetch(&mut broker, &url, hop, purpose, parent);
        match fetched {
            Ok(r) if !r.complete => {
                outcome.failed = true;
                notes.push(format!(
                    "Incomplete response from {host}; no original accepted"
                ));
            }
            Ok(r) if [301, 302, 303, 307, 308].contains(&r.status) => {
                if redirects >= 5 {
                    outcome.blocked = true;
                    notes.push("Redirect limit reached".into());
                    continue;
                }
                if let Some(next) = r.redirect.and_then(|s| url.join(&s).ok()) {
                    if let Ok(next) = policy::validate_https_url(next.as_str()) {
                        if next
                            .host_str()
                            .is_some_and(|h| hosts.iter().any(|s| s == h))
                        {
                            queue.push_front((
                                next,
                                hop,
                                redirects + 1,
                                RequestPurpose::Redirect,
                                request_sequence,
                            ));
                        } else {
                            outcome.blocked = true;
                            notes.push("Redirect leaves selected hosts".into());
                        }
                    } else {
                        outcome.blocked = true;
                        notes.push("Redirect violates the HTTPS URL policy".into());
                    }
                } else {
                    outcome.failed = true;
                    notes.push(format!("Redirect from {host} has no valid destination"));
                }
            }
            Ok(r) if r.status == 429 => {
                outcome.exhausted = true;
                notes.push("Website rate limit reached; no retries".into());
                break;
            }
            Ok(r) if r.status == 200 => {
                if media_type(&r.content_type).as_deref() != Some("text/html")
                    && media_type(&r.content_type).as_deref() != Some("text/plain")
                {
                    outcome.failed = true;
                    notes.push(format!(
                        "Unsupported response type from {host}; no searchable derivative accepted"
                    ));
                    continue;
                }
                let Ok(raw) = std::str::from_utf8(&r.body) else {
                    outcome.failed = true;
                    notes.push(format!(
                        "Non-UTF-8 response from {host}; no searchable derivative accepted"
                    ));
                    continue;
                };
                let (text, links) = static_page(raw.as_bytes(), &r.content_type, &url)
                    .expect("media type and UTF-8 were checked above");
                if hop < hops {
                    for link in links {
                        if queue.len() < 500
                            && link
                                .host_str()
                                .is_some_and(|h| hosts.iter().any(|s| s == h))
                        {
                            queue.push_back((
                                link,
                                hop + 1,
                                0,
                                RequestPurpose::Link,
                                request_sequence,
                            ));
                        }
                    }
                }
                pages.push(Page {
                    request_sequence: request_sequence.ok_or_else(|| {
                        Error::Validation("Fetched response lacks accounting".into())
                    })?,
                    url: url.to_string(),
                    bytes: r.body,
                    text,
                });
            }
            Ok(r) if [204, 205].contains(&r.status) => {
                notes.push(format!(
                    "HTTP {} from {host}; no source content returned",
                    r.status
                ));
            }
            Ok(r) => {
                outcome.failed = true;
                notes.push(format!("HTTP {} from {host}", r.status));
            }
            Err(e) => {
                outcome.record_error(&e);
                notes.push(e.to_string());
                if outcome.exhausted {
                    break;
                }
            }
        }
    }
    let elapsed_milliseconds = trace.monotonic.elapsed().as_millis() as u64;
    if elapsed_milliseconds > seconds * 1000 {
        outcome.exhausted = true;
        notes.push("Observed collection time exceeded the configured limit".into());
    }
    let state = outcome.state(pages.is_empty());
    let ended_at = trace.stamp();
    Ok(CollectionResult {
        mode,
        started_at: trace.started_at,
        ended_at,
        elapsed_milliseconds,
        trace: trace.requests,
        responses: trace.responses,
        pages,
        requests: broker.requests_used(),
        state,
        notes,
    })
}

#[derive(Default)]
#[cfg(test)]
struct Outcome {
    exhausted: bool,
    blocked: bool,
    failed: bool,
}
#[cfg(test)]
impl Outcome {
    fn record_error(&mut self, error: &Error) {
        match error {
            Error::QuotaExhausted(_) => self.exhausted = true,
            Error::Blocked(_) | Error::Validation(_) => self.blocked = true,
            _ => self.failed = true,
        }
    }
    fn state(&self, empty: bool) -> crate::domain::JobState {
        use crate::domain::JobState::*;
        if self.exhausted {
            QuotaExhausted
        } else if self.blocked {
            Blocked
        } else if self.failed {
            Failed
        } else if empty {
            SuccessfulNoResults
        } else {
            Successful
        }
    }
}

#[cfg(test)]
#[path = "collection_tests.rs"]
mod tests;
