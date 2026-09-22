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
    })
}
fn redirect(target: Option<&str>) -> Result<Fetched> {
    Ok(Fetched {
        status: 302,
        body: vec![],
        redirect: target.map(str::to_owned),
        content_type: "text/html".into(),
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
        }),
        Ok(Fetched {
            status: 200,
            body: vec![0xff],
            redirect: None,
            content_type: "text/plain".into(),
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
