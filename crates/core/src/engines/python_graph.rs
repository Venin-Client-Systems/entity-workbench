//! Fixed app-owned graph adapter. Not configured by the normal application.
// The exclusive coordinator is the only intended future caller. Keep source
// compiled and tested while normal production graph execution stays unavailable.
#![allow(dead_code)]
use super::{ocr::digest, CancellationToken};
use crate::{require, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod observation;
#[cfg(test)]
pub(crate) use observation::TestObservation;

const RECIPE: &str = "python-graph-job-v1";
const MANIFEST: &str = "4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822";
const REQUEST_LIMIT: usize = 1024 * 1024;
const RESULT_LIMIT: usize = 128 * 1024;
const WRAPPER_LIMIT: usize = 64 * 1024;
const VERSIONS: &[u8] = include_bytes!("../../../../workers/python/runtime_versions.json");

/// Cannot be deserialized, cloned from a path or obtained from Runtime/env/PATH.
/// Explicit app-resource configuration verifies the whole pinned prefix first;
/// execution verifies it again. No caller is installed by this source slice.
pub(crate) struct VerifiedGraphRuntime {
    prefix: PathBuf,
    #[cfg(test)]
    observation: std::sync::OnceLock<std::sync::Arc<TestObservation>>,
}

impl VerifiedGraphRuntime {
    #[cfg(test)]
    pub(crate) fn observe(self, observation: std::sync::Arc<TestObservation>) -> Self {
        self.attach_observation(observation)
            .expect("Native observation already attached");
        self
    }
    /// Test host attaches once, before admitting any job; never replaces the capability.
    #[cfg(test)]
    pub(crate) fn attach_observation(
        &self,
        observation: std::sync::Arc<TestObservation>,
    ) -> Result<()> {
        self.observation
            .set(observation)
            .map_err(|_| Error::Validation("Native observation already attached".into()))
    }
    pub(crate) fn from_app_engines(root: &Path, cancel: &CancellationToken) -> Result<Self> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            let prefix = root.join("python");
            super::supervision::python::verify_prefix(&prefix, MANIFEST, 11_320, cancel)
                .map_err(unavailable)?;
            Ok(Self {
                prefix,
                #[cfg(test)]
                observation: std::sync::OnceLock::new(),
            })
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            let _ = (root, cancel);
            Err(Error::Blocked(
                "Fixed graph runtime is unavailable on this platform".into(),
            ))
        }
    }

    pub(crate) fn execute(
        &self,
        scratch_root: &Path,
        request: &[u8],
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            use super::supervision::python;
            python::cancelled(cancel)?;
            let binding = Binding::new(request)?;
            #[cfg(test)]
            if let Some(observation) = self.observation.get() {
                observation.assignment(&binding.job_id, &binding.nonce, request)?;
            }
            let assignment = serde_json::to_vec(&serde_json::json!({
                "schema_version": 1, "recipe": RECIPE, "job_id": binding.job_id,
                "prefix": self.prefix, "manifest_sha256": MANIFEST,
                "capture_nonce": binding.nonce, "request_identity": binding.request,
            }))?;
            let assets: [python::AssetInput<'_>; 6] = [
                (
                    "code/graph_worker.py",
                    include_bytes!("../../../../workers/python/graph_worker.py"),
                    WRAPPER_LIMIT,
                ),
                (
                    "code/runtime_support.py",
                    include_bytes!("../../../../workers/python/runtime_support.py"),
                    WRAPPER_LIMIT,
                ),
                (
                    "code/graph_path.py",
                    include_bytes!("../../../../workers/python/graph_path.py"),
                    WRAPPER_LIMIT,
                ),
                ("input/runtime-versions.json", VERSIONS, WRAPPER_LIMIT),
                ("input/assignment.json", &assignment, WRAPPER_LIMIT),
                ("input/graph-request.json", request, REQUEST_LIMIT),
            ];
            python::execute_graph(
                &self.prefix,
                scratch_root,
                &assets,
                cancel,
                #[cfg(test)]
                self.observation.get().map(std::sync::Arc::as_ref),
                |job| {
                    let wrapper = python::read_verified(
                        &job.join("scratch/result.json"),
                        WRAPPER_LIMIT as u64,
                        cancel,
                    )?;
                    let graph = python::read_verified(
                        &job.join("scratch/graph-result.json"),
                        RESULT_LIMIT as u64,
                        cancel,
                    )?;
                    let graph = binding.accept(&wrapper, graph)?;
                    #[cfg(test)]
                    if let Some(observation) = self.observation.get() {
                        observation.accepted(&wrapper, &graph)?;
                    }
                    Ok(graph)
                },
            )
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            let _ = (scratch_root, request, cancel);
            Err(Error::Blocked(
                "Fixed graph runtime is unavailable on this platform".into(),
            ))
        }
    }
}

pub(super) fn unavailable(error: Error) -> Error {
    match error {
        // Cancellation retains its existing typed outcome and exact supervisor
        // message; never infer it by parsing a formatted error string.
        Error::Blocked(_) | Error::Interrupted(_) => error,
        _ => Error::Blocked("App-local graph runtime verification failed".into()),
    }
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    bytes: usize,
    sha256: String,
}
impl Identity {
    fn of(raw: &[u8]) -> Self {
        Self {
            bytes: raw.len(),
            sha256: digest(raw),
        }
    }
}

// Read only the correlation header. Graph topology and canonical authority stay
// in graph_path.parse_request and the owned GraphAttempt publication validator.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestHeader<'a> {
    schema_version: u32,
    recipe: &'a str,
    policy: &'a str,
    nonce: &'a str,
    workspace_revision: u64,
    snapshot_sha256: &'a str,
    engine: &'a str,
    engine_version: &'a str,
    runtime_manifest_sha256: &'a str,
    source_id: serde::de::IgnoredAny,
    target_id: serde::de::IgnoredAny,
    nodes: serde::de::IgnoredAny,
    edges: serde::de::IgnoredAny,
}

struct Binding {
    job_id: String,
    nonce: String,
    request: Identity,
}
impl Binding {
    fn new(request: &[u8]) -> Result<Self> {
        require(
            !request.is_empty() && request.len() <= REQUEST_LIMIT,
            "Graph request exceeds fixed bound",
        )?;
        let header: RequestHeader<'_> = serde_json::from_slice(request)?;
        let nonce = uuid::Uuid::parse_str(header.nonce)
            .map_err(|_| Error::Validation("Invalid graph capture nonce".into()))?;
        require(
            nonce.to_string() == header.nonce
                && nonce.get_version_num() == 4
                && nonce.get_variant() == uuid::Variant::RFC4122
                && header.runtime_manifest_sha256 == MANIFEST,
            "Graph capture correlation mismatch",
        )?;
        Ok(Self {
            job_id: uuid::Uuid::new_v4().to_string(),
            nonce: header.nonce.into(),
            request: Identity::of(request),
        })
    }
    fn accept(&self, wrapper: &[u8], graph: Vec<u8>) -> Result<Vec<u8>> {
        let checked = (|| {
            require(
                !wrapper.is_empty()
                    && wrapper.len() <= WRAPPER_LIMIT
                    && !graph.is_empty()
                    && graph.len() <= RESULT_LIMIT,
                "Graph worker output exceeds bound",
            )?;
            let result: Receipt = serde_json::from_slice(wrapper)?;
            let versions: BTreeMap<String, String> = serde_json::from_slice(VERSIONS)?;
            require(
                result.schema_version == 1
                    && result.recipe == RECIPE
                    && result.job_id == self.job_id
                    && result.manifest_sha256 == MANIFEST
                    && result.python_version == "3.13.15"
                    && result.isolated
                    && result.no_site
                    && result.no_bytecode
                    && result.verified_paths
                    && result.checks.versions == versions
                    && result.checks.imported_modules == ["networkx"]
                    && result.checks.backend_metadata_checked
                    && result.checks.capture_nonce == self.nonce
                    && result.checks.request_identity == self.request
                    && result.checks.result_identity == Identity::of(&graph),
                "Graph worker correlation mismatch",
            )?;
            Ok(())
        })();
        checked.map_err(|_: Error| {
            Error::InvalidWorkerResult("Fixed graph receipt rejected".into())
        })?;
        // Preserve duplicates, formatting and every raw byte for the store's
        // authoritative validation. Never deserialize/re-serialize this result.
        Ok(graph)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    recipe: String,
    job_id: String,
    manifest_sha256: String,
    python_version: String,
    isolated: bool,
    no_site: bool,
    no_bytecode: bool,
    verified_paths: bool,
    checks: Checks,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Checks {
    #[serde(deserialize_with = "unique_versions")]
    versions: BTreeMap<String, String>,
    imported_modules: Vec<String>,
    backend_metadata_checked: bool,
    capture_nonce: String,
    request_identity: Identity,
    result_identity: Identity,
}

pub(super) fn unique_versions<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error> {
    struct Versions;
    impl<'de> serde::de::Visitor<'de> for Versions {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("unique selected version fields")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((name, version)) = map.next_entry::<String, String>()? {
                if result.len() >= 58 || result.insert(name, version).is_some() {
                    return Err(serde::de::Error::custom(
                        "Duplicate or excess version field",
                    ));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Versions)
}

#[cfg(test)]
mod tests;
