//! Synthetic transport scripts exercise collector decisions without network access.
use super::*;
use crate::domain::JobState;

struct ScriptedTransport {
    script: VecDeque<(String, u32, Result<Fetched>)>,
    used: u32,
}
impl Transport for ScriptedTransport {
    fn fetch(&mut self, url: &Url, hop: u32) -> Result<Fetched> {
        let (expected, expected_hop, result) = self.script.pop_front().expect("unexpected request");
        assert_eq!(url.as_str(), expected);
        assert_eq!(hop, expected_hop);
        self.used += 1;
        result
    }
    fn requests_used(&self) -> u32 {
        self.used
    }
    fn elapsed_seconds(&self) -> u64 {
        0
    }
}
fn response(status: u16, body: &str) -> Result<Fetched> {
    Ok(Fetched {
        status,
        body: body.as_bytes().to_vec(),
        redirect: None,
        content_type: "text/html; charset=utf-8".into(),
        complete: true,
    })
}
fn redirect(target: Option<&str>) -> Result<Fetched> {
    Ok(Fetched {
        status: 302,
        body: vec![],
        redirect: target.map(str::to_owned),
        content_type: "text/html".into(),
        complete: true,
    })
}
fn run(script: Vec<(&str, u32, Result<Fetched>)>, requests: u32, hops: u32) -> CollectionResult {
    collect_with_transport(
        vec![Url::parse("https://example.com/start").unwrap()],
        hops,
        requests,
        600,
        ScriptedTransport {
            script: script
                .into_iter()
                .map(|(path, hop, result)| (format!("https://example.com{path}"), hop, result))
                .collect(),
            used: 0,
        },
    )
    .unwrap()
}
#[test]
fn robots_denial_unavailability_and_rate_limit_remain_distinct() {
    let denied = run(
        vec![(
            "/robots.txt",
            0,
            response(200, "User-agent: *\nDisallow: /"),
        )],
        50,
        2,
    );
    assert!(matches!(denied.state, JobState::Blocked));
    assert_eq!(denied.requests, 1);
    let unavailable = run(
        vec![(
            "/robots.txt",
            0,
            Err(Error::Network("synthetic DNS timeout".into())),
        )],
        50,
        2,
    );
    assert!(matches!(unavailable.state, JobState::Failed));
    let server_failure = run(vec![("/robots.txt", 0, response(503, ""))], 50, 2);
    assert!(matches!(server_failure.state, JobState::Failed));
    let rate_limit = run(vec![("/robots.txt", 0, response(429, ""))], 50, 2);
    assert!(matches!(rate_limit.state, JobState::QuotaExhausted));
    assert!(rate_limit.pages.is_empty());
    let limit = run(
        vec![("/robots.txt", 0, Err(Error::QuotaExhausted("time".into())))],
        50,
        2,
    );
    assert!(matches!(limit.state, JobState::QuotaExhausted));
}
#[test]
fn unsupported_response_and_tls_failure_are_not_successful_empty_results() {
    for bad in [
        Ok(Fetched {
            status: 200,
            body: vec![],
            redirect: None,
            content_type: "application/pdf".into(),
            complete: true,
        }),
        Ok(Fetched {
            status: 200,
            body: vec![0xff],
            redirect: None,
            content_type: "text/plain".into(),
            complete: true,
        }),
        Err(Error::Network("synthetic TLS failure".into())),
        response(500, "server error"),
    ] {
        let result = run(
            vec![("/robots.txt", 0, response(404, "")), ("/start", 0, bad)],
            50,
            2,
        );
        assert!(matches!(result.state, JobState::Failed));
        assert!(result.pages.is_empty());
        assert!(!result.notes.is_empty());
    }
    let empty = run(
        vec![
            ("/robots.txt", 0, response(404, "")),
            ("/start", 0, response(204, "")),
        ],
        50,
        2,
    );
    assert!(matches!(empty.state, JobState::SuccessfulNoResults));
}
#[test]
fn redirect_scope_and_missing_destinations_fail_without_fetching_them() {
    for target in [
        "https://outside.example/",
        "http://example.com/",
        "https://user:password@example.com/",
        "https://example.com:444/",
    ] {
        let result = run(
            vec![
                ("/robots.txt", 0, response(404, "")),
                ("/start", 0, redirect(Some(target))),
            ],
            50,
            2,
        );
        assert!(matches!(result.state, JobState::Blocked));
        assert_eq!(result.requests, 2);
    }
    let missing = run(
        vec![
            ("/robots.txt", 0, response(404, "")),
            ("/start", 0, redirect(None)),
        ],
        50,
        2,
    );
    assert!(matches!(missing.state, JobState::Failed));
    let permitted = run(
        vec![
            ("/robots.txt", 0, response(404, "")),
            ("/start", 0, redirect(Some("/destination#fragment"))),
            ("/destination", 0, response(200, "<p>Synthetic source</p>")),
        ],
        50,
        0,
    );
    assert!(matches!(permitted.state, JobState::Successful));
    assert_eq!(permitted.pages[0].url, "https://example.com/destination");
}
#[test]
fn request_limits_keep_completed_sources_without_silent_extra_requests() {
    let start = "<a href='/next'>Next source</a>";
    let result = run(
        vec![
            ("/robots.txt", 0, response(404, "")),
            ("/start", 0, response(200, start)),
        ],
        2,
        2,
    );
    assert!(matches!(result.state, JobState::QuotaExhausted));
    assert_eq!(result.requests, 2);
    assert_eq!(result.pages.len(), 1);
    let robots_only = run(vec![("/robots.txt", 0, response(404, ""))], 1, 2);
    assert!(matches!(robots_only.state, JobState::QuotaExhausted));
    assert!(robots_only.pages.is_empty());
    let limited = run(
        vec![
            ("/robots.txt", 0, response(404, "")),
            ("/start", 0, response(200, start)),
            ("/next", 1, response(429, "")),
        ],
        50,
        2,
    );
    assert!(matches!(limited.state, JobState::QuotaExhausted));
    assert_eq!(limited.pages.len(), 1);
}
#[test]
fn discovery_expands_only_allowed_links_to_the_reviewed_hop_limit() {
    let result = run(vec![
        ("/robots.txt", 0, response(200, "User-agent: *\nAllow: /")),
        ("/start", 0, response(200, "<p>First source</p><a href='/next'>Further lead</a><a href='https://outside.example/'>Offsite</a><a href='https://example.com:444/'>Wrong port</a>")),
        ("/next", 1, response(200, "<p>New identifier SAMPLE-000021</p><a href='/last'>Further source</a>")),
        ("/last", 2, response(200, "<p>Related fictional record</p><a href='/too-far'>Stop here</a>")),
    ], 50, 2);
    assert!(matches!(result.state, JobState::Successful));
    assert_eq!(result.requests, 4);
    assert_eq!(result.pages.len(), 3);
    assert!(result.pages[1].text.contains("SAMPLE-000021"));
    assert_eq!(result.pages[2].url, "https://example.com/last");
}

fn assert_trace_consistent(result: &CollectionResult) {
    assert_eq!(result.mode, AcquisitionMode::Synthetic);
    assert_eq!(result.requests as usize, result.trace.len());
    let parse = |value: &str| {
        assert_eq!(value.len(), 20);
        assert!(value.ends_with('Z'));
        chrono::DateTime::parse_from_rfc3339(value).unwrap()
    };
    let mut previous_end = parse(&result.started_at);
    let ended_at = parse(&result.ended_at);
    for (index, request) in result.trace.iter().enumerate() {
        assert_eq!(request.sequence as usize, index);
        assert_eq!(request.method, "GET");
        let start = parse(&request.started_at);
        let end = parse(&request.ended_at);
        assert!(previous_end <= start && start <= end && end <= ended_at);
        previous_end = end;
        // Collection has captured bytes; only canonical storage can bind evidence IDs.
        assert!(request.original_evidence_id.is_none());
        let bodies: Vec<_> = result
            .responses
            .iter()
            .filter(|body| body.sequence == request.sequence)
            .collect();
        if request.outcome == FetchOutcome::Fetched {
            assert_eq!(bodies.len(), 1);
            assert_eq!(request.body_bytes, Some(bodies[0].bytes.len() as u64));
            assert_eq!(
                request.body_sha256,
                Some(crate::store::hash(&bodies[0].bytes))
            );
        } else {
            assert!(bodies.is_empty());
            assert!(request.body_sha256.is_none());
            assert!(request.body_bytes.is_none());
        }
    }
    assert_eq!(
        result.responses.len(),
        result
            .trace
            .iter()
            .filter(|request| request.outcome == FetchOutcome::Fetched)
            .count()
    );
}

#[test]
fn trace_preserves_redirect_then_link_ancestry_and_separate_originals() {
    let destination = "<p>Fictional programme</p><a href='/lead'>Related project</a>";
    let lead = "<p>Fictional further lead</p>";
    let result = run(
        vec![
            ("/robots.txt", 0, response(404, "")),
            ("/start", 0, redirect(Some("/destination#ignored"))),
            ("/destination", 0, response(200, destination)),
            ("/lead", 1, response(200, lead)),
        ],
        50,
        2,
    );
    assert!(matches!(result.state, JobState::Successful));
    assert_trace_consistent(&result);
    assert_eq!(result.trace.len(), 4);
    let ancestry: Vec<_> = result
        .trace
        .iter()
        .map(|request| (request.purpose, request.parent_request, request.hop))
        .collect();
    assert_eq!(
        ancestry,
        vec![
            (RequestPurpose::AccessReview, None, 0),
            (RequestPurpose::Seed, None, 0),
            (RequestPurpose::Redirect, Some(1), 0),
            (RequestPurpose::Link, Some(2), 1),
        ]
    );
    assert_eq!(
        result.trace[1].redirect_url.as_deref(),
        Some("https://example.com/destination")
    );
    assert_eq!(result.pages[0].request_sequence, 2);
    assert_eq!(result.pages[1].request_sequence, 3);
    assert_eq!(result.responses[0].bytes, b"");
    assert_eq!(result.responses[1].bytes, b"");
    assert_eq!(result.responses[2].bytes, destination.as_bytes());
    assert_eq!(result.responses[3].bytes, lead.as_bytes());
}

#[test]
fn trace_keeps_complete_empty_unsupported_and_error_bodies_without_promoting_pages() {
    for (status, bytes, content_type) in [
        (204, b"".as_slice(), "text/plain"),
        (200, b"%PDF-SYNTHETIC".as_slice(), "application/pdf"),
        (200, &[0xff, 0xfe], "text/plain"),
        (500, b"Synthetic server failure".as_slice(), "text/plain"),
        (429, b"Synthetic source quota".as_slice(), "text/plain"),
    ] {
        let result = run(
            vec![
                ("/robots.txt", 0, response(404, "")),
                (
                    "/start",
                    0,
                    Ok(Fetched {
                        status,
                        body: bytes.to_vec(),
                        redirect: None,
                        content_type: content_type.into(),
                        complete: true,
                    }),
                ),
            ],
            50,
            2,
        );
        assert_trace_consistent(&result);
        assert!(result.pages.is_empty());
        assert_eq!(result.trace[0].http_status, Some(404));
        assert_eq!(result.trace[0].body_bytes, Some(0));
        assert_eq!(result.trace[0].body_sha256, Some(crate::store::hash(b"")));
        assert_eq!(result.trace[1].outcome, FetchOutcome::Fetched);
        assert_eq!(result.trace[1].http_status, Some(status));
        assert_eq!(result.trace[1].media_type.as_deref(), Some(content_type));
        assert_eq!(result.responses[1].bytes, bytes);
        match status {
            204 => assert!(matches!(result.state, JobState::SuccessfulNoResults)),
            429 => assert!(matches!(result.state, JobState::QuotaExhausted)),
            _ => assert!(matches!(result.state, JobState::Failed)),
        }
    }
}

#[test]
fn trace_does_not_hash_or_promote_incomplete_or_oversized_bodies() {
    for (bytes, complete) in [
        (b"<p>Interrupted synthetic body".to_vec(), false),
        (vec![b'x'; PAGE_BYTES + 1], true),
    ] {
        let result = run(
            vec![
                ("/robots.txt", 0, response(404, "")),
                (
                    "/start",
                    0,
                    Ok(Fetched {
                        status: 200,
                        body: bytes,
                        redirect: None,
                        content_type: "text/html; charset=utf-8".into(),
                        complete,
                    }),
                ),
            ],
            50,
            2,
        );
        assert!(matches!(result.state, JobState::Failed));
        assert_trace_consistent(&result);
        assert_eq!(result.requests, 2);
        assert!(result.pages.is_empty());
        assert_eq!(result.responses.len(), 1);
        assert_eq!(result.trace[1].outcome, FetchOutcome::Incomplete);
        assert_eq!(result.trace[1].http_status, Some(200));
        assert_eq!(result.trace[1].media_type.as_deref(), Some("text/html"));
    }
    let robots_incomplete = run(
        vec![(
            "/robots.txt",
            0,
            Ok(Fetched {
                status: 200,
                body: b"User-agent: *\nAllow: /".to_vec(),
                redirect: None,
                content_type: "text/plain".into(),
                complete: false,
            }),
        )],
        50,
        2,
    );
    assert!(matches!(robots_incomplete.state, JobState::Failed));
    assert_trace_consistent(&robots_incomplete);
    assert_eq!(robots_incomplete.requests, 1);
    assert!(robots_incomplete.pages.is_empty());
    assert!(robots_incomplete.responses.is_empty());
}

#[test]
fn trace_has_no_phantom_request_when_robots_consumes_the_budget() {
    let result = run(vec![("/robots.txt", 0, response(404, ""))], 1, 2);
    assert!(matches!(result.state, JobState::QuotaExhausted));
    assert_trace_consistent(&result);
    assert_eq!(result.requests, 1);
    assert_eq!(result.trace[0].purpose, RequestPurpose::AccessReview);
    assert_eq!(result.responses.len(), 1);
    assert!(result.pages.is_empty());
}

struct ReservationRefusal {
    used: u32,
    called_seed: bool,
}
impl Transport for ReservationRefusal {
    fn fetch(&mut self, url: &Url, hop: u32) -> Result<Fetched> {
        assert_eq!(hop, 0);
        if self.used == 0 {
            assert_eq!(url.as_str(), "https://example.com/robots.txt");
            self.used = 1;
            response(404, "")
        } else {
            assert!(!self.called_seed, "No retry after refused reservation");
            assert_eq!(url.as_str(), "https://example.com/start");
            self.called_seed = true;
            Err(Error::QuotaExhausted(
                "Synthetic deadline reached before reservation".into(),
            ))
        }
    }
    fn requests_used(&self) -> u32 {
        self.used
    }
    fn elapsed_seconds(&self) -> u64 {
        0
    }
}

#[test]
fn trace_excludes_a_reservation_refused_before_charge() {
    let result = collect_with_transport(
        vec![Url::parse("https://example.com/start").unwrap()],
        2,
        50,
        600,
        ReservationRefusal {
            used: 0,
            called_seed: false,
        },
    )
    .unwrap();
    assert!(matches!(result.state, JobState::QuotaExhausted));
    assert_trace_consistent(&result);
    assert_eq!(result.requests, 1);
    assert_eq!(result.trace[0].purpose, RequestPurpose::AccessReview);
    assert!(result.pages.is_empty());
    assert!(result
        .notes
        .iter()
        .any(|note| note.contains("before reservation")));
}

#[test]
fn trace_counts_charged_failures_without_claiming_a_response() {
    for (error, expected) in [
        (
            Error::Network("Synthetic DNS failure".into()),
            FetchOutcome::Failed,
        ),
        (
            Error::Validation("Synthetic destination rejection".into()),
            FetchOutcome::Blocked,
        ),
        (
            Error::QuotaExhausted("Synthetic deadline after reservation".into()),
            FetchOutcome::Blocked,
        ),
    ] {
        let result = run(
            vec![
                ("/robots.txt", 0, response(404, "")),
                ("/start", 0, Err(error)),
            ],
            50,
            2,
        );
        assert_trace_consistent(&result);
        assert_eq!(result.requests, 2);
        assert_eq!(result.responses.len(), 1);
        assert!(result.pages.is_empty());
        let failed = &result.trace[1];
        assert_eq!(failed.outcome, expected);
        assert_eq!(failed.purpose, RequestPurpose::Seed);
        assert!(failed.http_status.is_none());
        assert!(failed.media_type.is_none());
        assert!(failed.redirect_url.is_none());
        assert!(!matches!(result.state, JobState::SuccessfulNoResults));
    }
}

#[test]
fn trace_retains_denied_redirect_metadata_without_fetching_or_echoing_credentials() {
    for (target, retained) in [
        ("https://outside.example/", Some("https://outside.example/")),
        ("https://user:synthetic-secret@example.com/", None),
        ("http://example.com/", None),
        ("https://example.com:444/", None),
    ] {
        let result = run(
            vec![
                ("/robots.txt", 0, response(404, "")),
                ("/start", 0, redirect(Some(target))),
            ],
            50,
            2,
        );
        assert!(matches!(result.state, JobState::Blocked));
        assert_trace_consistent(&result);
        assert_eq!(result.requests, 2);
        assert_eq!(result.trace[1].redirect_url.as_deref(), retained);
        assert_eq!(result.trace[1].outcome, FetchOutcome::Fetched);
        assert!(result.pages.is_empty());
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(!serialized.contains("synthetic-secret"));
    }
}
