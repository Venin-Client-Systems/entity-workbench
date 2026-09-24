//! Fixed offline specimens for real-core UI tests. Never compiled in release builds.
use super::*;
use crate::collection_receipt::{AcquisitionMode, RequestPurpose};

const AT: &str = "2026-01-01T00:00:00Z";

fn request(sequence: u32, path: &str, purpose: RequestPurpose) -> RequestReceipt {
    RequestReceipt {
        sequence,
        url: format!("https://archive.example{path}"),
        method: "GET".into(),
        purpose,
        parent_request: None,
        hop: 0,
        started_at: AT.into(),
        ended_at: AT.into(),
        outcome: FetchOutcome::Fetched,
        http_status: Some(200),
        media_type: Some("text/plain".into()),
        redirect_url: None,
        body_sha256: None,
        body_bytes: None,
        original_evidence_id: None,
    }
}

fn result(state: JobState) -> CollectionResult {
    CollectionResult {
        pages: vec![],
        requests: 0,
        state,
        notes: vec!["Fixed synthetic fixture; no network requests were made".into()],
        mode: AcquisitionMode::Synthetic,
        started_at: AT.into(),
        ended_at: AT.into(),
        elapsed_milliseconds: 0,
        trace: vec![],
        responses: vec![],
    }
}

fn fetched(
    result: &mut CollectionResult,
    mut request: RequestReceipt,
    bytes: &[u8],
    text: Option<&str>,
) {
    request.body_sha256 = Some(hash(bytes));
    request.body_bytes = Some(bytes.len() as u64);
    if let Some(text) = text {
        result.pages.push(Page {
            request_sequence: request.sequence,
            url: request.url.clone(),
            bytes: bytes.to_vec(),
            text: text.into(),
        });
    }
    result.responses.push(ResponseBody {
        sequence: request.sequence,
        bytes: bytes.to_vec(),
    });
    result.trace.push(request);
    result.requests += 1;
}

impl Workspace {
    /// Developer harness only: rejects any workspace with prior records or revisions.
    /// It accepts no fixture payload, SQL, remote address or arbitrary file contents.
    pub fn seed_collection_review(&mut self) -> Result<()> {
        let records: u64 = self
            .conn
            .query_row("SELECT count(*) FROM records", [], |r| r.get(0))?;
        require(
            self.revision()? == 0 && records == 0,
            "Collection review fixtures require a fresh empty workspace",
        )?;

        let (job, revision) =
            self.start_collection(vec!["https://archive.example/research".into()], 2, 4, 600)?;
        let mut collected = result(JobState::QuotaExhausted);
        let mut access = request(0, "/robots.txt", RequestPurpose::AccessReview);
        access.http_status = Some(404);
        fetched(&mut collected, access, b"", None);
        let mut seed = request(1, "/research", RequestPurpose::Seed);
        seed.http_status = Some(302);
        seed.redirect_url = Some("https://archive.example/programme".into());
        fetched(&mut collected, seed, b"", None);
        let mut redirected = request(2, "/programme", RequestPurpose::Redirect);
        redirected.parent_request = Some(1);
        let source_text = "Programme Alpha\n<script>window.syntheticExecution = true</script>\n<img src=\"https://unrequested.example/image\">";
        fetched(
            &mut collected,
            redirected,
            source_text.as_bytes(),
            Some(source_text),
        );
        let mut linked = request(3, "/project/alpha", RequestPurpose::Link);
        linked.parent_request = Some(2);
        linked.hop = 1;
        fetched(
            &mut collected,
            linked,
            b"Synthetic project Alpha",
            Some("Synthetic project Alpha"),
        );
        collected
            .notes
            .push("Request limit stopped further expansion".into());
        self.finish_collection(job, revision, collected)?;

        // Structural receipt specimen for the valid parent sequence zero.
        let (job, revision) = self.start_collection(
            vec!["https://archive.example/first-parent".into()],
            2,
            50,
            600,
        )?;
        let mut collected = result(JobState::Successful);
        fetched(
            &mut collected,
            request(0, "/first-parent", RequestPurpose::Seed),
            b"Synthetic parent",
            Some("Synthetic parent"),
        );
        let mut child = request(1, "/first-child", RequestPurpose::Link);
        child.parent_request = Some(0);
        child.hop = 1;
        fetched(
            &mut collected,
            child,
            b"Synthetic child",
            Some("Synthetic child"),
        );
        self.finish_collection(job, revision, collected)?;

        let (job, revision) = self.start_collection(
            vec![
                "https://archive.example/empty".into(),
                "https://archive.example/document.pdf".into(),
            ],
            2,
            50,
            600,
        )?;
        let mut collected = result(JobState::SuccessfulNoResults);
        let mut empty = request(0, "/empty", RequestPurpose::Seed);
        empty.http_status = Some(204);
        fetched(&mut collected, empty, b"", None);
        let mut unsupported = request(1, "/document.pdf", RequestPurpose::Seed);
        unsupported.media_type = Some("application/pdf".into());
        fetched(
            &mut collected,
            unsupported,
            b"synthetic unsupported PDF bytes",
            None,
        );
        self.finish_collection(job, revision, collected)?;

        for (path, state, outcome) in [
            ("/incomplete", JobState::Failed, FetchOutcome::Incomplete),
            ("/blocked", JobState::Blocked, FetchOutcome::Blocked),
            ("/failed", JobState::Failed, FetchOutcome::Failed),
        ] {
            let (job, revision) =
                self.start_collection(vec![format!("https://archive.example{path}")], 2, 50, 600)?;
            let mut collected = result(state);
            let mut attempt = request(0, path, RequestPurpose::Seed);
            if outcome != FetchOutcome::Incomplete {
                attempt.http_status = None;
                attempt.media_type = None;
            }
            attempt.outcome = outcome;
            collected.trace.push(attempt);
            collected.requests = 1;
            self.finish_collection(job, revision, collected)?;
        }

        // Exercise the real failure publication path; no fabricated canonical receipt.
        let (job, revision) = self.start_collection(
            vec!["https://archive.example/partial-retention".into()],
            2,
            50,
            600,
        )?;
        let mut collected = result(JobState::Successful);
        fetched(
            &mut collected,
            request(0, "/partial-retention", RequestPurpose::Seed),
            b"Expected complete original",
            None,
        );
        collected.responses[0].bytes = b"Deliberate synthetic retention mismatch".to_vec();
        self.finish_collection(job, revision, collected)?;

        // A fixed interrupted legacy attempt has no invented trace.
        let (mut job, _) = self.start_collection(
            vec!["https://archive.example/interrupted".into()],
            2,
            50,
            600,
        )?;
        job.state = JobState::Failed;
        job.detail = "Synthetic interrupted legacy attempt; acquisition receipt unavailable".into();
        self.change(None, "collection.demo.interrupted", false, |conn| {
            put(conn, "job", &job.id, &job)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_is_offline_consistent_and_rejects_nonempty_workspaces() {
        let temp = tempfile::TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path().join("synthetic")).unwrap();
        workspace.seed_collection_review().unwrap();
        let view = workspace.view().unwrap();
        assert_eq!(view.jobs.len(), 8);
        for job in &view.jobs {
            if job.queries[0].ends_with("/interrupted") {
                assert!(workspace.collection_receipt(&job.id).is_err());
            } else {
                let receipt = workspace.collection_receipt(&job.id).unwrap();
                assert_eq!(receipt.mode, AcquisitionMode::Synthetic);
                receipt.validate().unwrap();
            }
        }
        let revision = workspace.revision().unwrap();
        assert!(workspace.seed_collection_review().is_err());
        assert_eq!(workspace.revision().unwrap(), revision);
        assert_eq!(workspace.view().unwrap().jobs.len(), 8);
    }
}
