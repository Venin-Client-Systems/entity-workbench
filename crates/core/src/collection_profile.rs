//! Pure benchmark-specific publisher-access admission, not a general collection plan.
//! Embedded public references provide no permission or implicit network selection.
//! No acquisition authority.
//! Existing collection policies, replay, transport and public commands are unchanged.
use crate::{require, Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::{Host, Url};

pub const PROFILE_ID: &str = "selected-publisher-access-v1";
pub const BENCHMARK_SHA256: &str =
    "9fc07bacc9fa73dbccd6352e3cd2f938a46b7f2b4a3f3ed51726dfb0f83840f6";
pub const MAX_ACCESS_URLS: usize = 50;
pub const MAX_URL_BYTES: usize = 2048;
const BENCHMARK: &[u8] = include_bytes!("../../../docs/discovery/public-benchmark.v1.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileLimits {
    pub max_hops: u32,
    pub max_requests: u32,
    pub max_seconds: u64,
}
impl ProfileLimits {
    fn within(self, ceiling: Self) -> bool {
        self.max_hops <= ceiling.max_hops
            && self.max_requests <= ceiling.max_requests
            && self.max_seconds > 0
            && self.max_seconds <= ceiling.max_seconds
    }
}

/// Untrusted local plan data, not a new command or persisted record schema.
/// There is deliberately no field carrying query text, case contents or headers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublisherAccessInput {
    pub benchmark_sha256: String,
    pub task_id: String,
    pub publisher_id: String,
    pub publisher_domain: String,
    pub seed_url: String,
    pub local_query_sha256: String,
    pub session_id: String,
    pub access_urls: Vec<String>,
    pub effective_limits: ProfileLimits,
}

/// Construction is validation-only; no Deserialize, mutable accessor or public fields.
/// This does not establish a cold corpus, reviewer approval, DNS safety or a lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPublisherAccessPlan {
    input: PublisherAccessInput,
    plan_sha256: String,
}

/// Syntax/scope proof only. It is not a transport ticket or request reservation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileUrl(Url);
impl ProfileUrl {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub fn host(&self) -> &str {
        self.0.host_str().expect("validated domain URL")
    }
}

/// Explicitly selected URL scope; local query/session metadata is absent.
/// Caller-selected path text is still a disclosure. This is not an access approval.
#[derive(Debug, Serialize)]
pub struct ProfileUrlDisclosure<'a> {
    pub publisher_domain: &'a str,
    pub includes_subdomains: bool,
    pub seed_url: &'a str,
    pub access_urls: &'a [String],
    pub local_query_sent: bool,
}

impl ValidatedPublisherAccessPlan {
    pub fn validate(input: PublisherAccessInput) -> Result<Self> {
        require(
            input.benchmark_sha256 == BENCHMARK_SHA256 && sha256(BENCHMARK) == BENCHMARK_SHA256,
            "Publisher profile requires the exact frozen benchmark",
        )?;
        require(
            identifier(&input.task_id) && identifier(&input.publisher_id),
            "Invalid frozen task or publisher identifier",
        )?;
        require(
            input.publisher_domain.len() <= 253 && !input.publisher_domain.is_empty(),
            "Invalid publisher domain",
        )?;
        require(
            input.local_query_sha256.len() == 64
                && input
                    .local_query_sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "Invalid local query digest",
        )?;
        require(
            input.session_id.len() == 36
                && uuid::Uuid::parse_str(&input.session_id)
                    .is_ok_and(|id| !id.is_nil() && id.to_string() == input.session_id),
            "Invalid canonical session UUID",
        )?;
        require(
            input.access_urls.len() <= MAX_ACCESS_URLS,
            "Too many selected access URLs",
        )?;
        // The compiled document is byte-pinned before its bounded, trusted projection.
        // This is not a second general benchmark-file validator or a caller-loaded file.
        let frozen: FrozenBenchmark = serde_json::from_slice(BENCHMARK)?;
        let task = frozen
            .tasks
            .iter()
            .find(|task| task.id == input.task_id)
            .ok_or_else(|| Error::Validation("Unknown frozen task".into()))?;
        let publisher = frozen
            .publishers
            .iter()
            .find(|publisher| publisher.id == task.publisher_id)
            .ok_or_else(|| Error::Validation("Frozen task lacks a publisher".into()))?;
        require(
            input.publisher_id == task.publisher_id
                && input.publisher_domain == publisher.domain
                && input.seed_url == publisher.seed_url
                && input.local_query_sha256 == sha256(task.query.as_bytes()),
            "Plan differs from the frozen task, publisher, seed or local query",
        )?;
        require(
            input.effective_limits.within(frozen.limits),
            "Publisher profile limits exceed the frozen ceiling",
        )?;
        scoped_url(&input.seed_url, &input.publisher_domain)?;
        let mut selected = std::collections::BTreeSet::new();
        for raw in &input.access_urls {
            scoped_url(raw, &input.publisher_domain)?;
            require(selected.insert(raw), "Duplicate selected access URL")?;
        }
        let plan_sha256 = plan_digest(&input)?;
        Ok(Self { input, plan_sha256 })
    }

    pub fn input(&self) -> &PublisherAccessInput {
        &self.input
    }
    pub fn plan_sha256(&self) -> &str {
        &self.plan_sha256
    }
    pub fn url_disclosure(&self) -> ProfileUrlDisclosure<'_> {
        ProfileUrlDisclosure {
            publisher_domain: &self.input.publisher_domain,
            includes_subdomains: true,
            seed_url: &self.input.seed_url,
            access_urls: &self.input.access_urls,
            local_query_sent: false,
        }
    }
    /// Check outbound syntax and publisher suffix only; lineage, purpose, access
    /// decisions, DNS/address validation, quotas and networking remain unimplemented.
    pub fn validate_destination_syntax(&self, raw: &str) -> Result<ProfileUrl> {
        scoped_url(raw, &self.input.publisher_domain)
    }
    /// An access URL additionally has to be an exact member of the immutable plan.
    /// No claim is made that the selected resource actually contains terms or robots.
    pub fn selected_access_url(&self, raw: &str) -> Result<ProfileUrl> {
        require(
            self.input
                .access_urls
                .iter()
                .any(|selected| selected == raw),
            "Access URL was not explicitly selected",
        )?;
        self.validate_destination_syntax(raw)
    }
    /// Metadata-only reduction; no budget is started, spent, refreshed or refunded.
    pub fn tighten(&self, limits: ProfileLimits) -> Result<Self> {
        require(
            limits.within(self.input.effective_limits),
            "Profile limits may only tighten",
        )?;
        let mut input = self.input.clone();
        input.effective_limits = limits;
        Self::validate(input)
    }
}

fn scoped_url(raw: &str, domain: &str) -> Result<ProfileUrl> {
    require(
        !raw.is_empty()
            && raw.len() <= MAX_URL_BYTES
            && raw.is_ascii()
            && !raw
                .bytes()
                .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
            && !raw.contains('\\'),
        "Invalid bounded canonical publisher URL",
    )?;
    let url = Url::parse(raw).map_err(|_| Error::Validation("Invalid publisher URL".into()))?;
    let host = match url.host() {
        Some(Host::Domain(host)) => host,
        _ => {
            return Err(Error::Validation(
                "Publisher URL requires a DNS domain".into(),
            ))
        }
    };
    require(
        host.len() <= 253
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && label.as_bytes()[0].is_ascii_alphanumeric()
                    && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                    && label
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            }),
        "Invalid publisher DNS hostname",
    )?;
    require(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443)
            && url.query().is_none()
            && url.fragment().is_none()
            && url.as_str() == raw
            && url[..url::Position::BeforePath] == format!("https://{host}"),
        "Publisher URL must be canonical credential-free HTTPS without query, fragment or explicit port",
    )?;
    require(
        host == domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.')),
        "URL is outside the frozen publisher domain and subdomains",
    )?;
    Ok(ProfileUrl(url))
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn plan_digest(input: &PublisherAccessInput) -> Result<String> {
    #[derive(Serialize)]
    struct Payload<'a> {
        profile: &'static str,
        input: &'a PublisherAccessInput,
    }
    // Input bounds cap this allocation before serialization; access order is retained.
    Ok(sha256(&serde_json::to_vec(&Payload {
        profile: PROFILE_ID,
        input,
    })?))
}

#[derive(Deserialize)]
struct FrozenBenchmark {
    limits: ProfileLimits,
    publishers: Vec<FrozenPublisher>,
    tasks: Vec<FrozenTask>,
}
#[derive(Deserialize)]
struct FrozenPublisher {
    id: String,
    domain: String,
    seed_url: String,
}
#[derive(Deserialize)]
struct FrozenTask {
    id: String,
    publisher_id: String,
    query: String,
}

#[cfg(test)]
#[path = "collection_profile_tests.rs"]
mod tests;
