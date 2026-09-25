use super::*;
const AT: i64 = 1_700_000_000_000;
fn fixture() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn input() -> CollectionInput {
    CollectionInput {
        urls: vec!["https://example.com/selected?q=synthetic".into()],
        max_hops: 2,
        max_requests: 5,
        max_seconds: 60,
    }
}
fn queued(w: &mut Workspace, protocol: CollectionProtocol) -> DurableCollectionJob {
    w.queue_collection_protocol(input(), &id(), AT, protocol)
        .unwrap()
}
#[test]
fn disclosure_digest_binds_order_queries_limits_and_normalized_scope_without_side_effects() {
    let p = preview(input()).unwrap();
    assert_eq!(p.selected_hosts, ["example.com"]);
    assert_eq!(p.robots_urls, ["https://example.com/robots.txt"]);
    assert!(!p.disclosure.automatic_case_contents);
    assert_eq!(confirmed(input(), &p.preview_sha256).unwrap(), p.input);
    let mut normalized = input();
    normalized.urls[0] = "https://EXAMPLE.com:443/selected?q=synthetic".into();
    assert_eq!(preview(normalized).unwrap(), p);
    let mut altered = input();
    altered.max_requests += 1;
    assert!(confirmed(altered, &p.preview_sha256).is_err());
    let mut altered = input();
    altered.urls[0].push_str("&changed=true");
    assert!(confirmed(altered, &p.preview_sha256).is_err());
    let mut multi = input();
    multi.urls.push("https://other.example/".into());
    let before = preview(multi.clone()).unwrap();
    multi.urls.reverse();
    assert_ne!(
        preview(multi).unwrap().preview_sha256,
        before.preview_sha256
    );
    for bad in ["http://example.com", "https://user:password@example.com/"] {
        let mut value = input();
        value.urls[0] = bad.into();
        assert!(preview(value).is_err());
    }
    let mut value = input();
    value.max_seconds = 601;
    assert!(preview(value).is_err());
}
#[test]
fn v3_request_identity_binds_policy_mode_input_and_preserves_historical_bytes() {
    let (_temp, mut w) = fixture();
    let mut old = Vec::new();
    for protocol in [
        CollectionProtocol::FoundationV1,
        CollectionProtocol::SyntheticV2,
    ] {
        let job = queued(&mut w, protocol);
        let raw: String = w
            .conn
            .query_row(
                "SELECT body FROM records WHERE kind='collection_run' AND id=?",
                [&job.id],
                |r| r.get(0),
            )
            .unwrap();
        old.push((job.id, raw));
    }
    let key = id();
    let a = w
        .queue_collection_protocol(input(), &key, AT, CollectionProtocol::SyntheticV3)
        .unwrap();
    let rev = w.revision().unwrap();
    assert_eq!(
        w.queue_collection_protocol(input(), &key, AT + 2, CollectionProtocol::SyntheticV3)
            .unwrap(),
        a
    );
    for protocol in [
        CollectionProtocol::NativeV3,
        CollectionProtocol::SyntheticV2,
    ] {
        assert!(w
            .queue_collection_protocol(input(), &key, AT + 3, protocol)
            .is_err());
    }
    let mut changed = input();
    changed.max_hops = 0;
    assert!(w
        .queue_collection_protocol(changed, &key, AT + 3, CollectionProtocol::SyntheticV3)
        .is_err());
    assert_eq!(w.revision().unwrap(), rev);
    for (key, bytes) in old {
        assert!(w.inspect_collection_run(&key).unwrap().run.record_version < 3);
        let after: String = w
            .conn
            .query_row(
                "SELECT body FROM records WHERE kind='collection_run' AND id=?",
                [&key],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(after, bytes);
    }
    let mut malformed = a.clone();
    malformed.collector_policy = "unknown-policy".into();
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            params![serde_json::to_string(&malformed).unwrap(), a.id],
        )
        .unwrap();
    assert!(w.inspect_collection_run(&a.id).is_err());
}
#[test]
fn run_pages_are_revision_and_size_bound_ordered_complete_and_reject_forged_positions() {
    let (_temp, mut w) = fixture();
    let a = queued(&mut w, CollectionProtocol::SyntheticV3);
    let b = queued(&mut w, CollectionProtocol::FoundationV1);
    let c = queued(&mut w, CollectionProtocol::NativeV3);
    let revision = w.revision().unwrap();
    let first = w
        .page_collection_runs(
            &CollectionRunPageRequest {
                page_size: 2,
                cursor: None,
            },
            Some(revision),
        )
        .unwrap();
    assert_eq!(first.scope_count, 3);
    assert_eq!(
        first.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        [a.id.clone(), b.id.clone()]
    );
    let next = first.next_cursor.clone().unwrap();
    let last = w
        .page_collection_runs(
            &CollectionRunPageRequest {
                page_size: 2,
                cursor: Some(next.clone()),
            },
            None,
        )
        .unwrap();
    assert_eq!(last.rows[0].id, c.id);
    assert!(last.next_cursor.is_none());
    assert!(w
        .page_collection_runs(
            &CollectionRunPageRequest {
                page_size: 1,
                cursor: Some(next.clone())
            },
            None
        )
        .is_err());
    let mut decoded = Cursor::decode(
        &next,
        &hash(
            &serde_json::to_vec(&(
                1u32,
                "collection_run",
                "canonical_sequence_ascending",
                revision,
                2u32,
            ))
            .unwrap(),
        ),
    )
    .unwrap();
    decoded.id = a.id;
    assert!(w
        .page_collection_runs(
            &CollectionRunPageRequest {
                page_size: 2,
                cursor: Some(decoded.encode().unwrap())
            },
            None
        )
        .is_err());
    queued(&mut w, CollectionProtocol::SyntheticV3);
    assert!(w
        .page_collection_runs(
            &CollectionRunPageRequest {
                page_size: 2,
                cursor: Some(next)
            },
            None
        )
        .is_err());
    assert!(w
        .page_collection_runs(
            &CollectionRunPageRequest {
                page_size: 2,
                cursor: None
            },
            Some(revision)
        )
        .is_err());
    for size in [0, 26, u32::MAX] {
        assert!(w
            .page_collection_runs(
                &CollectionRunPageRequest {
                    page_size: size,
                    cursor: None
                },
                None
            )
            .is_err());
    }
    assert!(w
        .page_collection_runs(
            &CollectionRunPageRequest {
                page_size: 1,
                cursor: Some("a".repeat(1025))
            },
            None
        )
        .is_err());
}
#[test]
fn bounded_reads_fail_for_oversized_or_key_substituted_runs_and_never_return_a_prefix() {
    let (_temp, mut w) = fixture();
    let a = queued(&mut w, CollectionProtocol::SyntheticV3);
    let b = queued(&mut w, CollectionProtocol::SyntheticV3);
    let raw = serde_json::to_string(&b).unwrap();
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            params![raw, a.id],
        )
        .unwrap();
    assert!(w.inspect_collection_run(&a.id).is_err());
    assert!(w
        .page_collection_runs(
            &CollectionRunPageRequest {
                page_size: 25,
                cursor: None
            },
            None
        )
        .is_err());
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            params![
                serde_json::to_string(&" ".repeat(MAX_RECORD_BYTES + 1)).unwrap(),
                a.id
            ],
        )
        .unwrap();
    assert!(w.inspect_collection_run(&a.id).is_err());
    assert!(w
        .page_collection_runs(
            &CollectionRunPageRequest {
                page_size: 25,
                cursor: None
            },
            None
        )
        .is_err());
    assert!(response_bound(&"x".repeat(RESPONSE_BYTES)).is_err());
}
#[test]
fn standalone_and_legacy_live_controls_refuse_before_creating_any_record() {
    let (_temp, mut w) = fixture();
    let before = w.revision().unwrap();
    let p = preview(input()).unwrap();
    assert!(w
        .dispatch(Command::QueueCollection {
            input: input(),
            preview_sha256: p.preview_sha256,
            request_key: id()
        })
        .is_err());
    assert!(w.collect_web(input().urls, 2, 5, 60).is_err());
    assert!(crate::collection::collect(input().urls, 2, 5, 60).is_err());
    assert_eq!(w.revision().unwrap(), before);
    let run = queued(&mut w, CollectionProtocol::SyntheticV3);
    let inspection = w.inspect_collection_run(&run.id).unwrap();
    assert_eq!(
        inspection.availability,
        CollectionAvailability::StandaloneUnavailable
    );
    assert!(
        !inspection.controls.can_cancel
            && !inspection.controls.can_resume
            && !inspection.controls.can_retry_settlement
    );
    assert!(w
        .dispatch(Command::CancelCollection {
            job_id: run.id,
            expected_generation: 1
        })
        .is_err());
}

#[test]
fn acknowledgement_lookup_refuses_corrupt_missing_and_historical_bindings_without_fallback() {
    let (_temp, mut w) = fixture();
    let old = queued(&mut w, CollectionProtocol::SyntheticV2);
    assert!(w
        .existing_collection_request(&input(), &old.request_key, CollectionProtocol::SyntheticV3)
        .is_err());
    assert!(w
        .existing_collection_request(&input(), &id(), CollectionProtocol::SyntheticV3)
        .unwrap()
        .is_none());
    let run = queued(&mut w, CollectionProtocol::SyntheticV3);
    let revision = w.revision().unwrap();
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run_key' AND id=?",
            params![serde_json::to_string(&old.id).unwrap(), run.request_key],
        )
        .unwrap();
    assert!(w
        .existing_collection_request(&input(), &run.request_key, CollectionProtocol::SyntheticV3)
        .is_err());
    assert!(w
        .queue_collection_protocol(
            input(),
            &run.request_key,
            AT,
            CollectionProtocol::SyntheticV3
        )
        .is_err());
    assert_eq!(w.revision().unwrap(), revision);
    let count: u64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM records WHERE kind='collection_run'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}
