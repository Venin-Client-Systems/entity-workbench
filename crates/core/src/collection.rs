//! The only live HTTP transport. All requests are explicit, bounded and DNS-pinned.
use crate::{
    policy::{self, DiscoveryBudget},
    require, Error, Result,
};
use reqwest::{blocking::Client, redirect::Policy};
use scraper::{Html, Selector};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    io::Read,
    net::{SocketAddr, ToSocketAddrs},
    sync::mpsc,
    time::{Duration, Instant},
};
use url::Url;
const PAGE_BYTES: usize = 2 * 1024 * 1024;
const AGENT: &str = "EntityWorkbench";
#[derive(Debug)]
pub struct Page {
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
}
pub fn validate_seeds(seeds: &[String]) -> Result<Vec<Url>> {
    require(
        !seeds.is_empty() && seeds.len() <= 10,
        "Select one to ten seed URLs",
    )?;
    seeds
        .iter()
        .map(|raw| {
            require(raw.len() <= 2048, "Seed URL exceeds 2048 bytes")?;
            let mut url =
                Url::parse(raw).map_err(|_| Error::Validation("Invalid seed URL".into()))?;
            require(
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port_or_known_default() == Some(443),
                "Seeds require credential-free HTTPS on port 443",
            )?;
            url.set_fragment(None);
            Ok(url)
        })
        .collect()
}
struct Broker {
    hosts: Vec<String>,
    budget: DiscoveryBudget,
    started: Instant,
    last_request: Option<Instant>,
}
struct Fetched {
    status: u16,
    body: Vec<u8>,
    redirect: Option<String>,
    content_type: String,
}
impl Broker {
    fn fetch(&mut self, url: &Url, hop: u32) -> Result<Fetched> {
        self.budget
            .reserve(hop, self.started.elapsed().as_secs(), u32::MAX)?;
        let remaining = self
            .budget
            .seconds
            .saturating_sub(self.started.elapsed().as_secs());
        require(remaining > 0, "Collection time limit exhausted")?;
        let host = url
            .host_str()
            .ok_or_else(|| Error::Validation("Missing host".into()))?
            .to_string();
        require(
            self.hosts.contains(&host),
            "Redirect or link leaves the selected hosts",
        )?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let dns_host = host.clone();
        std::thread::spawn(move || {
            let result = (dns_host.as_str(), 443)
                .to_socket_addrs()
                .map(|i| i.collect::<Vec<SocketAddr>>());
            let _ = sender.send(result);
        });
        let addresses = receiver
            .recv_timeout(Duration::from_secs(5.min(remaining)))
            .map_err(|_| Error::Blocked("DNS resolution timed out".into()))??;
        let hosts: Vec<_> = self.hosts.iter().map(String::as_str).collect();
        policy::validate_destination(
            url.as_str(),
            &addresses.iter().map(|a| a.ip()).collect::<Vec<_>>(),
            &hosts,
        )?;
        if let Some(last) = self.last_request {
            let delay = Duration::from_secs(1).saturating_sub(last.elapsed());
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
        }
        let remaining = self
            .budget
            .seconds
            .saturating_sub(self.started.elapsed().as_secs());
        if remaining == 0 {
            return Err(Error::Blocked("Collection time limit exhausted".into()));
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .referer(false)
            .https_only(true)
            .timeout(Duration::from_secs(15.min(remaining)))
            .connect_timeout(Duration::from_secs(5.min(remaining)))
            .resolve_to_addrs(&host, &addresses)
            .user_agent("EntityWorkbench/0.1 (analyst-directed public collection)")
            .build()
            .map_err(|_| Error::Blocked("TLS transport could not initialise".into()))?;
        self.last_request = Some(Instant::now());
        let response = client
            .get(url.as_str())
            .header("Accept", "text/html, text/plain;q=0.9")
            .send()
            .map_err(|_| {
                Error::Blocked("HTTPS request failed; TLS verification remains enabled".into())
            })?;
        let status = response.status().as_u16();
        let redirect = response
            .headers()
            .get("location")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();
        if let Some(size) = response.content_length() {
            require(
                size <= PAGE_BYTES as u64,
                "Response exceeds page size limit",
            )?;
        }
        let mut body = vec![];
        response
            .take((PAGE_BYTES + 1) as u64)
            .read_to_end(&mut body)?;
        require(body.len() <= PAGE_BYTES, "Response exceeds page size limit")?;
        Ok(Fetched {
            status,
            body,
            redirect,
            content_type,
        })
    }
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
        .filter(|u| u.scheme() == "https" && u.username().is_empty() && u.password().is_none())
        .map(|mut u| {
            u.set_fragment(None);
            u
        })
        .collect();
    (text, links)
}
pub fn collect(
    seeds: Vec<String>,
    hops: u32,
    requests: u32,
    seconds: u64,
) -> Result<CollectionResult> {
    let seeds = validate_seeds(&seeds)?;
    require(
        hops <= 2 && requests > 0 && requests <= 50 && seconds > 0 && seconds <= 600,
        "Collection bounds exceed policy",
    )?;
    let mut broker = Broker {
        hosts: seeds
            .iter()
            .filter_map(|s| s.host_str().map(str::to_owned))
            .collect(),
        budget: DiscoveryBudget {
            hops,
            requests,
            seconds,
            used: 0,
        },
        started: Instant::now(),
        last_request: None,
    };
    let mut queue: VecDeque<_> = seeds.into_iter().map(|s| (s, 0, 0)).collect();
    let mut visited = HashSet::new();
    let mut robots: BTreeMap<String, String> = BTreeMap::new();
    let mut pages = vec![];
    let mut notes = vec![];
    let mut exhausted = false;
    let mut blocked = false;
    let mut failed = false;
    while let Some((url, hop, redirects)) = queue.pop_front() {
        if visited.len() >= 500 {
            exhausted = true;
            break;
        }
        if broker.budget.used >= requests || broker.started.elapsed().as_secs() >= seconds {
            exhausted = true;
            break;
        }
        if !visited.insert(url.to_string()) {
            continue;
        }
        let host = url.host_str().unwrap_or_default().to_owned();
        if !broker.hosts.contains(&host) {
            continue;
        }
        if !robots.contains_key(&host) {
            let mut robots_url = url.clone();
            robots_url.set_path("/robots.txt");
            robots_url.set_query(None);
            match broker.fetch(&robots_url, hop) {
                Ok(r) if r.status == 404 => {
                    robots.insert(host.clone(), String::new());
                }
                Ok(r) if r.status == 200 && r.body.len() <= 512_000 => {
                    robots.insert(host.clone(), String::from_utf8_lossy(&r.body).into_owned());
                }
                _ => {
                    blocked = true;
                    notes.push(format!(
                        "Robots policy unavailable for {host}; host blocked"
                    ));
                    robots.insert(host.clone(), "User-agent: *\nDisallow: /".into());
                }
            }
        }
        let policy = robots.get(&host).expect("robots policy populated");
        if !robotstxt::DefaultMatcher::default().one_agent_allowed_by_robots(
            policy,
            AGENT,
            url.as_str(),
        ) {
            blocked = true;
            notes.push(format!("Robots policy denied {}", url.path()));
            continue;
        }
        // A crawl-delay extension is treated conservatively until per-host scheduling
        // is implemented. No directive is silently ignored to increase throughput.
        if policy
            .lines()
            .any(|l| l.trim().to_ascii_lowercase().starts_with("crawl-delay:"))
        {
            blocked = true;
            notes.push(format!(
                "Crawl-delay policy requires manual scheduling for {host}"
            ));
            continue;
        }
        if broker.budget.used >= requests {
            exhausted = true;
            break;
        }
        match broker.fetch(&url, hop) {
            Ok(r) if [301, 302, 303, 307, 308].contains(&r.status) => {
                if redirects >= 5 {
                    blocked = true;
                    notes.push("Redirect limit reached".into());
                    continue;
                }
                if let Some(next) = r.redirect.and_then(|s| url.join(&s).ok()) {
                    if next
                        .host_str()
                        .is_some_and(|h| broker.hosts.iter().any(|s| s == h))
                    {
                        queue.push_front((next, hop, redirects + 1));
                    } else {
                        blocked = true;
                        notes.push("Redirect leaves selected hosts".into());
                    }
                }
            }
            Ok(r) if r.status == 429 => {
                exhausted = true;
                notes.push("Website rate limit reached; no retries".into());
                break;
            }
            Ok(r) if r.status == 200 => {
                if !r.content_type.starts_with("text/html")
                    && !r.content_type.starts_with("text/plain")
                {
                    notes.push(format!("Unsupported response type from {host}"));
                    continue;
                }
                let Ok(raw) = std::str::from_utf8(&r.body) else {
                    notes.push("Non-UTF-8 page retained only by a future encoding adapter".into());
                    continue;
                };
                let (text, links) = if r.content_type.starts_with("text/html") {
                    extract_html(raw, &url)
                } else {
                    (raw.to_owned(), vec![])
                };
                if hop < hops {
                    for link in links {
                        if queue.len() < 500
                            && link
                                .host_str()
                                .is_some_and(|h| broker.hosts.iter().any(|s| s == h))
                        {
                            queue.push_back((link, hop + 1, 0));
                        }
                    }
                }
                pages.push(Page {
                    url: url.to_string(),
                    bytes: r.body,
                    text,
                });
            }
            Ok(r) => {
                failed = true;
                notes.push(format!("HTTP {} from {host}", r.status));
            }
            Err(e) => {
                blocked = true;
                notes.push(e.to_string());
            }
        }
    }
    use crate::domain::JobState::*;
    let state = if exhausted {
        QuotaExhausted
    } else if blocked {
        Blocked
    } else if failed {
        Failed
    } else if pages.is_empty() {
        SuccessfulNoResults
    } else {
        Successful
    };
    Ok(CollectionResult {
        pages,
        requests: broker.budget.used,
        state,
        notes,
    })
}
