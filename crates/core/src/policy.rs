//! Policy validation is not an operating-system sandbox. No workers are launched here.
use crate::{require, Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr},
    path::{Component, Path},
};
use url::Url;

pub const MAX_IMPORT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkerRequest {
    pub protocol_version: u32,
    pub job_id: String,
    pub operation: WorkerOperation,
    pub inputs: Vec<String>,
    pub output: String,
    pub limits: WorkerLimits,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkerOperation {
    Parse,
    Ocr,
    Index,
    Search,
    Analyse,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkerLimits {
    pub seconds: u32,
    pub output_bytes: u64,
    pub pages: u32,
    pub pixels: u64,
    pub archive_members: u32,
    pub archive_depth: u32,
    pub expanded_bytes: u64,
}
pub fn safe_relative(value: &str) -> Result<()> {
    require(
        !value.is_empty() && !value.contains(['\\', ':', '\0']) && !value.starts_with('.'),
        "Unsafe relative worker path",
    )?;
    require(
        Path::new(value)
            .components()
            .all(|c| matches!(c, Component::Normal(_))),
        "Path traversal or absolute path is forbidden",
    )
}
pub fn validate_worker_request(bytes: &[u8]) -> Result<WorkerRequest> {
    require(
        bytes.len() <= MAX_MESSAGE_BYTES,
        "Worker request is too large",
    )?;
    let request: WorkerRequest = serde_json::from_slice(bytes)?;
    require(request.protocol_version == 1, "Unsupported worker protocol")?;
    uuid::Uuid::parse_str(&request.job_id)
        .map_err(|_| Error::Validation("Invalid job ID".into()))?;
    require(
        !request.inputs.is_empty() && request.inputs.len() <= 50,
        "Worker needs 1 to 50 explicit inputs",
    )?;
    for path in request
        .inputs
        .iter()
        .chain(std::iter::once(&request.output))
    {
        safe_relative(path)?;
    }
    let l = &request.limits;
    require(
        l.seconds > 0
            && l.seconds <= 600
            && l.output_bytes > 0
            && l.output_bytes <= 64 * 1024 * 1024
            && l.pages > 0
            && l.pages <= 10000
            && l.pixels > 0
            && l.pixels <= 100_000_000
            && l.archive_members <= 1000
            && l.archive_depth <= 4
            && l.expanded_bytes > 0
            && l.expanded_bytes <= 256 * 1024 * 1024,
        "Worker limits exceed application policy",
    )?;
    Ok(request)
}
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => public_v4(v),
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = v.segments();
            // Admit ordinary global unicast only; deny transition/special-use ranges.
            (s[0] & 0xe000) == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
        }
    }
}
fn public_v4(v: Ipv4Addr) -> bool {
    let [a, b, c, _] = v.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 100 && (64..=127).contains(&b))
        || (a == 198 && (b == 18 || b == 19))
        || (a == 192 && b == 0)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113))
}
/// Every redirect must be revalidated. A future transport must connect only to these
/// pinned addresses with TLS verification for the original host; validation alone
/// does not prevent rebinding. No transport is currently enabled.
pub fn validate_destination(raw: &str, resolved: &[IpAddr], allowed_hosts: &[&str]) -> Result<Url> {
    let url = Url::parse(raw).map_err(|_| Error::Validation("Invalid URL".into()))?;
    require(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443),
        "Only credential-free HTTPS on port 443 is allowed",
    )?;
    let host = url
        .host_str()
        .ok_or_else(|| Error::Validation("Missing host".into()))?;
    require(
        allowed_hosts.contains(&host),
        "Host is not in the reviewed connector manifest",
    )?;
    require(
        !resolved.is_empty() && resolved.iter().all(|v| public_ip(*v)),
        "Destination resolves to a private or special-use address",
    )?;
    Ok(url)
}
#[derive(Debug)]
pub struct DiscoveryBudget {
    pub hops: u32,
    pub requests: u32,
    pub seconds: u64,
    pub used: u32,
}
impl DiscoveryBudget {
    pub fn reserve(
        &mut self,
        hop: u32,
        elapsed_seconds: u64,
        provider_remaining: u32,
    ) -> Result<()> {
        if hop > self.hops
            || self.used >= self.requests
            || elapsed_seconds >= self.seconds
            || provider_remaining == 0
        {
            return Err(Error::Blocked("Discovery quota exhausted".into()));
        }
        self.used += 1;
        Ok(())
    }
}
