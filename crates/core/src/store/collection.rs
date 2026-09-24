//! Canonical publication of acquisition facts and immutable local export snapshots.
use super::*;
use crate::{
    collection::{CollectionResult, Page, ResponseBody},
    collection_receipt::{CollectionReceipt, FetchOutcome, RequestReceipt},
};
use std::collections::HashSet;

impl Workspace {
    pub fn collect_web(
        &mut self,
        urls: Vec<String>,
        hops: u32,
        requests: u32,
        seconds: u64,
    ) -> Result<()> {
        let (job, start_revision) = self.start_collection(urls, hops, requests, seconds)?;
        match crate::collection::collect(job.queries.clone(), hops, requests, seconds) {
            Ok(result) => self.finish_collection(job, start_revision, result),
            Err(error) => {
                let mut job = job;
                job.state = JobState::Failed;
                job.detail =
                    format!("Collection failed before a receipt could be completed: {error}");
                self.change(None, "collection.finish", false, |conn| {
                    put(conn, "job", &job.id, &job)
                })
            }
        }
    }

    fn start_collection(
        &mut self,
        urls: Vec<String>,
        hops: u32,
        requests: u32,
        seconds: u64,
    ) -> Result<(CollectionJob, u64)> {
        let urls = crate::collection::validate_seeds(&urls)?
            .into_iter()
            .map(|u| u.to_string())
            .collect();
        require(
            hops <= 2 && requests > 0 && requests <= 50 && seconds > 0 && seconds <= 600,
            "Collection limits exceed policy",
        )?;
        let start_revision = self.revision()?;
        let job = CollectionJob {
            id: id(),
            queries: urls,
            adapters: vec!["direct_web".into()],
            max_hops: hops,
            max_requests: requests,
            max_seconds: seconds,
            requests_used: 0,
            state: JobState::Running,
            detail: "Analyst selected direct website collection".into(),
        };
        self.change(None, "collection.start", false, |conn| {
            put(conn, "job", &job.id, &job)
        })?;
        Ok((job, start_revision))
    }

    fn finish_collection(
        &mut self,
        mut job: CollectionJob,
        start_revision: u64,
        result: CollectionResult,
    ) -> Result<()> {
        let mut receipt = CollectionReceipt {
            schema_version: 1,
            job_id: job.id.clone(),
            mode: result.mode,
            application_version: env!("CARGO_PKG_VERSION").into(),
            collector_policy: "direct-https-v1".into(),
            selected_urls: job.queries.clone(),
            max_hops: job.max_hops,
            max_requests: job.max_requests,
            max_seconds: job.max_seconds,
            requests_used: result.requests,
            started_at: result.started_at,
            ended_at: result.ended_at,
            elapsed_milliseconds: result.elapsed_milliseconds,
            time_limit_exceeded: result.elapsed_milliseconds > job.max_seconds * 1000,
            start_revision,
            retained_revision: 0,
            state: result.state,
            retention_complete: true,
            requests: result.trace,
            notes: result.notes,
        };
        let mut retained = HashSet::new();
        for response in result.responses {
            let request = receipt
                .requests
                .get_mut(response.sequence as usize)
                .ok_or_else(|| Error::Validation("Response has no request receipt".into()))?;
            require(
                retained.insert(response.sequence),
                "Duplicate retained response",
            )?;
            if self.retain_response(&job.id, request, response).is_err() {
                receipt.retention_complete = false;
                receipt.state = JobState::Failed;
                receipt
                    .notes
                    .push("Response retention failed; earlier originals remain available".into());
                break;
            }
        }
        let mut pages_retained = 0;
        if receipt.retention_complete {
            for page in result.pages {
                if self.promote_page(&receipt, page).is_err() {
                    receipt.retention_complete = false;
                    receipt.state = JobState::Failed;
                    receipt.notes.push(
                        "Text derivative publication failed; retained originals remain available"
                            .into(),
                    );
                    break;
                }
                pages_retained += 1;
            }
        }
        receipt.retained_revision = self.revision()? + 1;
        receipt.validate()?;
        self.verify_collection_originals(&receipt)?;
        require(
            serde_json::to_vec(&receipt)?.len() <= 1024 * 1024,
            "Collection receipt exceeds size limit",
        )?;
        job.requests_used = receipt.requests_used;
        job.state = receipt.state.clone();
        job.detail = format!(
            "{pages_retained} searchable pages retained. {}",
            receipt.notes.join("; ")
        );
        // An apparently finished job and its receipt cannot be published separately.
        self.change(None, "collection.finish", false, |conn| {
            put(conn, "collection_receipt", &job.id, &receipt)?;
            put(conn, "job", &job.id, &job)
        })
    }

    fn retain_response(
        &mut self,
        job_id: &str,
        request: &mut RequestReceipt,
        response: ResponseBody,
    ) -> Result<()> {
        let digest = hash(&response.bytes);
        require(
            request.outcome == FetchOutcome::Fetched
                && response.sequence == request.sequence
                && request.body_sha256.as_deref() == Some(&digest)
                && request.body_bytes == Some(response.bytes.len() as u64)
                && response.bytes.len() <= 2 * 1024 * 1024,
            "Response bytes do not match acquisition receipt",
        )?;
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM records WHERE kind='evidence' AND id=?",
                [&digest],
                |r| r.get(0),
            )
            .optional()?;
        let mut evidence: Evidence = if let Some(body) = existing {
            serde_json::from_str(&body)?
        } else {
            Evidence {
                id: digest.clone(),
                name: "http-response.bin".into(),
                sha256: digest.clone(),
                bytes: response.bytes.len() as u64,
                media_type: "application/octet-stream".into(),
                origin_group: digest.clone(),
                imported_at: now(),
                extraction_status: "acquisition_only".into(),
                text: None,
                acquisitions: vec![],
            }
        };
        // Complete zero-byte HTTP bodies are valid originals. Public document import still rejects empties.
        retain_original(&self.root, &evidence, &response.bytes)?;
        evidence.acquisitions.push(Acquisition {
            job_id: job_id.into(),
            url: request.url.clone(),
            retrieved_at: request.ended_at.clone(),
        });
        self.change(None, "collection.response", false, |conn| {
            put(conn, "evidence", &digest, &evidence)
        })?;
        request.original_evidence_id = Some(digest);
        Ok(())
    }

    fn promote_page(&mut self, receipt: &CollectionReceipt, page: Page) -> Result<()> {
        let request = receipt
            .requests
            .get(page.request_sequence as usize)
            .ok_or_else(|| Error::Validation("Page has no acquisition request".into()))?;
        let digest = hash(&page.bytes);
        require(
            request.outcome == FetchOutcome::Fetched
                && request.http_status == Some(200)
                && request.url == page.url
                && request.original_evidence_id.as_deref() == Some(&digest)
                && matches!(
                    request.media_type.as_deref(),
                    Some("text/html" | "text/plain")
                ),
            "Page differs from acquired original",
        )?;
        self.change(None, "collection.derivative", true, |conn| {
            let mut evidence: Evidence = get(conn, "evidence", &digest)?;
            evidence.text = Some(page.text);
            evidence.media_type = request.media_type.clone().unwrap_or_default();
            evidence.extraction_status = "static_text_only".into();
            put(conn, "evidence", &digest, &evidence)
        })
    }

    fn verify_collection_originals(&self, receipt: &CollectionReceipt) -> Result<()> {
        for request in &receipt.requests {
            if let Some(key) = &request.original_evidence_id {
                let evidence: Evidence = get(&self.conn, "evidence", key)?;
                require(
                    Some(&evidence.sha256) == request.body_sha256.as_ref()
                        && Some(evidence.bytes) == request.body_bytes
                        && evidence.acquisitions.iter().any(|a| {
                            a.job_id == receipt.job_id
                                && a.url == request.url
                                && a.retrieved_at == request.ended_at
                        }),
                    "Original acquisition binding is missing or inconsistent",
                )?;
                self.verify_original(&evidence)?;
            }
        }
        Ok(())
    }

    pub fn collection_receipt(&self, job_id: &str) -> Result<CollectionReceipt> {
        let job: CollectionJob = get(&self.conn, "job", job_id)?;
        let body: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM records WHERE kind='collection_receipt' AND id=?",
                [job_id],
                |r| r.get(0),
            )
            .optional()?;
        let body = body.ok_or_else(|| {
            Error::Blocked(
                "Acquisition receipt is unavailable for this legacy or interrupted job".into(),
            )
        })?;
        require(
            body.len() <= 1024 * 1024,
            "Collection receipt exceeds size limit",
        )?;
        let receipt: CollectionReceipt = serde_json::from_str(&body)?;
        receipt.validate()?;
        require(
            receipt.job_id == job.id
                && receipt.selected_urls == job.queries
                && receipt.requests_used == job.requests_used
                && receipt.max_hops == job.max_hops
                && receipt.max_requests == job.max_requests
                && receipt.max_seconds == job.max_seconds
                && serde_json::to_value(&receipt.state)? == serde_json::to_value(&job.state)?
                && receipt.retained_revision <= self.revision()?,
            "Receipt differs from canonical job",
        )?;
        self.verify_collection_originals(&receipt)?;
        Ok(receipt)
    }

    pub fn export_collection(&mut self, job_id: &str) -> Result<Value> {
        let receipt = self.collection_receipt(job_id)?;
        require(
            receipt.retention_complete,
            "Incomplete collection retention cannot be exported as a complete bundle",
        )?;
        let export_id = id();
        let exports = self.root.join("exports");
        private_dir(&exports)?;
        let staging = exports.join(format!(".pending-{export_id}"));
        let target = exports.join(&export_id);
        fs::create_dir(&staging)?;
        let mut published = false;
        let result = (|| {
            private_dir(&staging)?;
            private_dir(&staging.join("originals"))?;
            let mut files = vec![];
            let mut copied = HashSet::new();
            for request in &receipt.requests {
                if let Some(key) = &request.original_evidence_id {
                    if !copied.insert(key) {
                        continue;
                    }
                    let evidence: Evidence = get(&self.conn, "evidence", key)?;
                    self.verify_original(&evidence)?;
                    let relative = format!("originals/{key}.bin");
                    let path = staging.join(&relative);
                    fs::copy(self.root.join("originals").join(key), &path)?;
                    require(
                        fs::metadata(&path)?.len() == evidence.bytes
                            && hash(&fs::read(&path)?) == evidence.sha256,
                        "Exported original changed during copy",
                    )?;
                    private_file(&path, 0o400)?;
                    files.push(json!({"evidence_id":key,"path":relative,"sha256":evidence.sha256,"bytes":evidence.bytes}));
                }
            }
            let revision = self.revision()?;
            let bytes = serde_json::to_vec_pretty(
                &json!({"schema_version":1,"snapshot_revision":revision,
                "receipt":receipt,"originals":files,"limitations":["Acquisition evidence only; not source-access approval or a discovery benchmark result","Inspect retained bytes as data; do not execute captured content"]}),
            )?;
            let path = staging.join("manifest.json");
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            // Windows can deny renaming a directory while a child file is open.
            drop(file);
            private_file(&path, 0o400)?;
            require(!target.exists(), "Export destination already exists")?;
            fs::rename(&staging, &target)?;
            published = true;
            let record = json!({"schema_version":1,"id":export_id,"job_id":job_id,"snapshot_revision":revision,
                "created_at":now(),"path":format!("exports/{export_id}/manifest.json"),"sha256":hash(&bytes)});
            self.change(None, "collection.export", false, |conn| {
                put(conn, "collection_export", &export_id, &record)
            })?;
            Ok(record)
        })();
        if result.is_err() {
            // Remove only names created by this operation; prior snapshots are untouched.
            let _ = fs::remove_dir_all(&staging);
            if published {
                let _ = fs::remove_dir_all(&target);
            }
        }
        result
    }
}

#[cfg(test)]
#[path = "collection_tests.rs"]
mod tests;

// Fixed offline UI fixtures are unavailable to release builds.
#[cfg(debug_assertions)]
#[path = "collection_demo.rs"]
mod demo;
