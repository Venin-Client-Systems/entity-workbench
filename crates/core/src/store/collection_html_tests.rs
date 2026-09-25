//! Real canonical storage coverage; no networking or unreviewed execution seam.
use super::*;
use crate::collection_transport::{Observation, Outcome, Phase, ResolvedCandidates, ResponseHead};

fn fixture() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn input() -> CollectionInput {
    CollectionInput {
        urls: vec!["https://html.collection.invalid/selected".into()],
        max_hops: 0,
        max_requests: 2,
        max_seconds: 60,
    }
}
fn observation(body: &[u8], status: u16, at: i64) -> Observation {
    Observation {
        outcome: Outcome::Complete {
            head: ResponseHead {
                status,
                media_type: Some(
                    if status == 200 {
                        "text/html"
                    } else {
                        "text/plain"
                    }
                    .into(),
                ),
                redirect_url: None,
                identity_encoding: true,
            },
            body: body.to_vec(),
        },
        phase: Phase::Body,
        elapsed_milliseconds: 1,
        observed_wall_ms: at,
        resolved: Some(ResolvedCandidates {
            addresses: vec!["1.1.1.1:443".parse().unwrap()],
            method: "synthetic_fixed_candidates",
            authoritative_complete_set: false,
        }),
        resolver_uncertainty: None,
        stop_observed: None,
        locally_quiescent: true,
    }
}
fn complete(w: &mut Workspace, protocol: CollectionProtocol, body: &[u8]) -> DurableCollectionJob {
    let at = chrono::Utc::now().timestamp_millis() - 1000;
    let owner = w.collection_ownership().unwrap();
    let job = w
        .queue_collection_protocol(input(), &id(), at, protocol)
        .unwrap();
    let ticket = w
        .start_durable_collection(&job.id, 1, &owner, at)
        .unwrap()
        .unwrap();
    for (status, bytes) in [(404, &b"missing robots"[..]), (200, body)] {
        let request = w
            .advance_durable_collection(&ticket, &owner, at)
            .unwrap()
            .unwrap();
        if protocol == CollectionProtocol::FoundationV1 {
            w.complete_durable_collection(
                &request,
                &CollectionResponse::Complete {
                    status,
                    content_type: if status == 200 {
                        "text/html"
                    } else {
                        "text/plain"
                    }
                    .into(),
                    location: None,
                    body: bytes.to_vec(),
                },
                &owner,
                at,
            )
            .unwrap();
        } else {
            w.settle_collection_transport(&request, &observation(bytes, status, at), &owner)
                .unwrap();
        }
    }
    assert!(w
        .advance_durable_collection(&ticket, &owner, at)
        .unwrap()
        .is_none());
    w.inspect_durable_collection(&job.id).unwrap()
}
fn raw(w: &Workspace, id: &str) -> String {
    w.conn
        .query_row(
            "SELECT body FROM records WHERE kind='collection_run' AND id=?",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn new_html_refusal_retains_complete_receipt_original_and_no_partial_derivative_across_restore() {
    let (temp, mut w) = fixture();
    let html = format!(
        "{}<a href='/never'>visible</a>{}",
        "<div>x".repeat(129),
        "</div>".repeat(129)
    );
    let before_observations = serde_json::to_vec(&w.view().unwrap().observations).unwrap();
    let run = complete(&mut w, CollectionProtocol::SyntheticV4, html.as_bytes());
    assert_eq!(run.schema_version, 4);
    assert_eq!(run.checkpoint.state, CollectionState::QuotaExhausted);
    assert_eq!(run.checkpoint.requests_used(), 2);
    assert_eq!(run.checkpoint.pages_retained, 0);
    assert!(run.checkpoint.frontier.is_empty());
    let response = w.inspect_collection_run(&run.id).unwrap();
    let original = response.requests[1].original.as_ref().unwrap();
    assert_eq!(original.sha256, hash(html.as_bytes()));
    let evidence = get_evidence(&w.conn, &original.evidence_id).unwrap();
    assert_eq!(read_original(&w.root, &evidence).unwrap(), html.as_bytes());
    assert!(evidence.text.is_none());
    assert_eq!(evidence.extraction_status, "acquisition_only");
    assert_eq!(evidence.acquisitions.len(), 1);
    let RequestProgress::Observed { receipt } = &response.requests[1].progress else {
        panic!()
    };
    assert!(matches!(
        receipt.fetch_record(),
        FetchRecord::Complete { status: 200, .. }
    ));
    assert_eq!(
        serde_json::to_vec(&w.view().unwrap().observations).unwrap(),
        before_observations
    );
    let bytes = raw(&w, &run.id);
    let revision = w.revision().unwrap();
    let backup = w.backup().unwrap();
    drop(w);
    let reopened = Workspace::open(temp.path().join("case")).unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    for current in [&reopened, &restored] {
        assert_eq!(current.revision().unwrap(), revision);
        assert_eq!(current.inspect_durable_collection(&run.id).unwrap(), run);
        assert_eq!(raw(current, &run.id), bytes);
        let retained = get_evidence(&current.conn, &original.evidence_id).unwrap();
        assert_eq!(
            serde_json::to_vec(&retained).unwrap(),
            serde_json::to_vec(&evidence).unwrap()
        );
        assert_eq!(
            read_original(&current.root, &retained).unwrap(),
            html.as_bytes()
        );
    }
}

#[test]
fn ordinary_historical_v1_v2_v3_replay_and_evidence_stay_byte_exact() {
    let (temp, mut w) = fixture();
    let html = b"<p>ordinary <b>text</b></p><svg><a href='/hidden'>hidden</a></svg>";
    let mut snapshots = Vec::new();
    for protocol in [
        CollectionProtocol::FoundationV1,
        CollectionProtocol::SyntheticV2,
        CollectionProtocol::SyntheticV3,
    ] {
        let run = complete(&mut w, protocol, html);
        assert_eq!(run.checkpoint.state, CollectionState::Successful);
        assert_eq!(run.checkpoint.pages_retained, 1);
        snapshots.push((raw(&w, &run.id), run));
    }
    let evidence = get_evidence(&w.conn, &hash(html)).unwrap();
    assert_eq!(evidence.text.as_deref(), Some("ordinary text"));
    assert_eq!(evidence.acquisitions.len(), 3);
    let revision = w.revision().unwrap();
    drop(w);
    let w = Workspace::open(temp.path().join("case")).unwrap();
    for (bytes, run) in snapshots {
        assert_eq!(w.inspect_durable_collection(&run.id).unwrap(), run);
        assert_eq!(raw(&w, &run.id), bytes);
    }
    assert_eq!(
        serde_json::to_vec(&get_evidence(&w.conn, &hash(html)).unwrap()).unwrap(),
        serde_json::to_vec(&evidence).unwrap()
    );
    assert_eq!(w.revision().unwrap(), revision);
}

#[test]
fn historical_pathological_record_is_refused_read_only_and_original_survives_backup() {
    let (temp, mut w) = fixture();
    let html = format!("{}visible{}", "<div>x".repeat(129), "</div>".repeat(129));
    let mut old = complete(&mut w, CollectionProtocol::SyntheticV4, html.as_bytes());
    // Construct the old successful interpretation of this synthetic body. Its
    // complete transport events are identical; v3 had no interpretation caps.
    // This is a compatibility specimen, not a claim of native acquisition.
    old.schema_version = 3;
    old.collector_policy = CollectionProtocol::SyntheticV3.policy().into();
    old.checkpoint.saw_quota = false;
    old.checkpoint.pages_retained = 1;
    old.checkpoint.state = CollectionState::Successful;
    let bytes = serde_json::to_string(&old).unwrap();
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            params![bytes, old.id],
        )
        .unwrap();
    let evidence = get_evidence(&w.conn, &hash(html.as_bytes())).unwrap();
    let revision = w.revision().unwrap();
    let error = w.inspect_durable_collection(&old.id).unwrap_err();
    assert!(matches!(error, Error::Blocked(_)));
    assert!(error
        .to_string()
        .contains("Historical HTML interpretation refused without rewriting"));
    assert_eq!(raw(&w, &old.id), bytes);
    assert_eq!(w.revision().unwrap(), revision);
    assert_eq!(
        serde_json::to_vec(&get_evidence(&w.conn, &evidence.id).unwrap()).unwrap(),
        serde_json::to_vec(&evidence).unwrap()
    );
    let backup = w.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert!(restored.inspect_durable_collection(&old.id).is_err());
    assert_eq!(raw(&restored, &old.id), bytes);
    assert_eq!(
        read_original(
            &restored.root,
            &get_evidence(&restored.conn, &evidence.id).unwrap()
        )
        .unwrap(),
        html.as_bytes()
    );
}

#[test]
fn v4_policy_preview_and_request_key_cannot_replay_v3_as_current_execution() {
    let (_temp, mut w) = fixture();
    let previous =
        crate::collection_api::preview_policy(input(), CollectionProtocol::NativeV3.policy())
            .unwrap();
    let current = crate::collection_api::preview(input()).unwrap();
    assert_ne!(previous.preview_sha256, current.preview_sha256);
    assert!(crate::collection_api::confirmed(input(), &previous.preview_sha256).is_err());
    assert!(crate::collection_api::confirmed(input(), &current.preview_sha256).is_ok());
    let key = id();
    let old = w
        .queue_collection_protocol(
            input(),
            &key,
            1_700_000_000_000,
            CollectionProtocol::SyntheticV3,
        )
        .unwrap();
    let revision = w.revision().unwrap();
    assert!(w
        .queue_collection_protocol(
            input(),
            &key,
            1_700_000_000_000,
            CollectionProtocol::SyntheticV4
        )
        .is_err());
    assert_eq!(w.inspect_durable_collection(&old.id).unwrap(), old);
    assert_eq!(w.revision().unwrap(), revision);
}
