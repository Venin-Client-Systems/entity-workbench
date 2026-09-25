use super::*;

fn input() -> PublisherAccessInput {
    PublisherAccessInput {
        benchmark_sha256: BENCHMARK_SHA256.into(),
        task_id: "discovery-01".into(),
        publisher_id: "nasa".into(),
        publisher_domain: "nasa.gov".into(),
        seed_url: "https://www.nasa.gov/missions/".into(),
        local_query_sha256: format!("{:x}", Sha256::digest(b"Artemis")),
        session_id: "11111111-1111-4111-8111-111111111111".into(),
        access_urls: vec![
            "https://www.nasa.gov/robots.txt".into(),
            "https://www.nasa.gov/terms/".into(),
        ],
        effective_limits: ProfileLimits {
            max_hops: 2,
            max_requests: 50,
            max_seconds: 600,
        },
    }
}

#[test]
fn independently_listed_frozen_publishers_and_query_classes_bind_exactly() {
    // Frozen public metadata only: no resources are acquired or permissions inferred.
    let examples = [
        (
            "discovery-01",
            "nasa",
            "nasa.gov",
            "https://www.nasa.gov/missions/",
            "Artemis",
        ),
        (
            "discovery-02",
            "nasa",
            "nasa.gov",
            "https://www.nasa.gov/missions/",
            "Webb",
        ),
        (
            "discovery-03",
            "nasa",
            "nasa.gov",
            "https://www.nasa.gov/missions/",
            "nasa.gov",
        ),
        (
            "discovery-04",
            "esa",
            "esa.int",
            "https://www.esa.int/Science_Exploration",
            "Euclid",
        ),
        (
            "discovery-07",
            "noaa",
            "noaa.gov",
            "https://www.noaa.gov/",
            "Ocean Exploration",
        ),
        (
            "discovery-10",
            "usgs",
            "usgs.gov",
            "https://www.usgs.gov/",
            "Earthquake Hazards",
        ),
        (
            "discovery-13",
            "cern",
            "home.cern",
            "https://home.cern/science/experiments",
            "ATLAS",
        ),
        (
            "discovery-16",
            "eso",
            "eso.org",
            "https://www.eso.org/public/teles-instr/",
            "Extremely Large Telescope",
        ),
        (
            "discovery-19",
            "british-museum",
            "britishmuseum.org",
            "https://www.britishmuseum.org/collection",
            "British Museum collection",
        ),
        (
            "discovery-22",
            "australian-museum",
            "australian.museum",
            "https://australian.museum/learn/",
            "Australian Museum Research Institute",
        ),
        (
            "discovery-25",
            "royal-society",
            "royalsociety.org",
            "https://royalsociety.org/",
            "Royal Society grants",
        ),
        (
            "discovery-28",
            "national-gallery",
            "nationalgallery.org.uk",
            "https://www.nationalgallery.org.uk/",
            "National Gallery research",
        ),
    ];
    for (task, publisher, domain, seed, query) in examples {
        let mut value = input();
        value.task_id = task.into();
        value.publisher_id = publisher.into();
        value.publisher_domain = domain.into();
        value.seed_url = seed.into();
        value.local_query_sha256 = format!("{:x}", Sha256::digest(query.as_bytes()));
        value.access_urls.clear();
        let plan = ValidatedPublisherAccessPlan::validate(value.clone()).unwrap();
        assert_eq!(plan.input(), &value);
        assert_eq!(
            plan.validate_destination_syntax(seed).unwrap().as_str(),
            seed
        );
    }
}

#[test]
fn frozen_binding_cannot_be_retargeted_or_normalized() {
    let mutations: Vec<fn(&mut PublisherAccessInput)> = vec![
        |v| v.benchmark_sha256 = "0".repeat(64),
        |v| v.task_id = "discovery-31".into(),
        |v| v.task_id = "discovery-02".into(),
        |v| v.publisher_id = "esa".into(),
        |v| v.publisher_domain = "gov".into(),
        |v| v.publisher_domain = "www.nasa.gov".into(),
        |v| v.seed_url = "https://www.nasa.gov/".into(),
        |v| v.seed_url = "https://www.nasa.gov/missions/#section".into(),
        |v| v.local_query_sha256 = format!("{:x}", Sha256::digest(b"artemis")),
        |v| v.local_query_sha256 = format!("{:x}", Sha256::digest(b"Artemis ")),
    ];
    for mutate in mutations {
        let mut value = input();
        mutate(&mut value);
        assert!(ValidatedPublisherAccessPlan::validate(value).is_err());
    }
}

#[test]
fn malformed_bounded_metadata_and_noncanonical_session_are_refused() {
    for id in ["", "DISCOVERY-01", "discovery-01\n", &"x".repeat(81)] {
        let mut value = input();
        value.task_id = id.into();
        assert!(ValidatedPublisherAccessPlan::validate(value).is_err());
    }
    for id in [
        "",
        "00000000-0000-0000-0000-000000000000",
        "{11111111-1111-4111-8111-111111111111}",
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
        "11111111111141118111111111111111",
    ] {
        let mut value = input();
        value.session_id = id.into();
        assert!(ValidatedPublisherAccessPlan::validate(value).is_err());
    }
    for digest in [
        "0".repeat(63),
        "0".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        let mut value = input();
        value.local_query_sha256 = digest;
        assert!(ValidatedPublisherAccessPlan::validate(value).is_err());
    }
}

#[test]
fn publisher_scope_uses_exact_domain_boundary_not_registrable_domain() {
    let plan = ValidatedPublisherAccessPlan::validate(input()).unwrap();
    for raw in [
        "https://nasa.gov/",
        "https://www.nasa.gov/a",
        "https://deep.test.nasa.gov/a",
    ] {
        assert!(plan.validate_destination_syntax(raw).is_ok(), "{raw}");
    }
    for raw in [
        "https://notnasa.gov/",
        "https://nasa.gov.attacker.example/",
        "https://nasa-gov.example/",
        "https://.nasa.gov/",
        "https://a..nasa.gov/",
        "https://_bad.nasa.gov/",
        "https://-bad.nasa.gov/",
        "https://bad-.nasa.gov/",
        "https://nasa.gov./",
    ] {
        assert!(plan.validate_destination_syntax(raw).is_err(), "{raw}");
    }
    let mut value = input();
    value.task_id = "discovery-13".into();
    value.publisher_id = "cern".into();
    value.publisher_domain = "home.cern".into();
    value.seed_url = "https://home.cern/science/experiments".into();
    value.local_query_sha256 = format!("{:x}", Sha256::digest(b"ATLAS"));
    value.access_urls.clear();
    let cern = ValidatedPublisherAccessPlan::validate(value).unwrap();
    assert!(cern
        .validate_destination_syntax("https://assets.home.cern/terms")
        .is_ok());
    for raw in ["https://cern/", "https://atlas.cern/", "https://www.cern/"] {
        assert!(cern.validate_destination_syntax(raw).is_err());
    }
}

#[test]
fn url_aliases_userinfo_query_fragment_and_ports_are_not_silently_normalized() {
    let plan = ValidatedPublisherAccessPlan::validate(input()).unwrap();
    for raw in [
        "http://www.nasa.gov/",
        "https://user@www.nasa.gov/",
        "https://@www.nasa.gov/",
        "https://user:secret@www.nasa.gov/",
        "https://www.nasa.gov:444/",
        "https://www.nasa.gov:443/",
        "https://www.nasa.gov/?q=Artemis",
        "https://www.nasa.gov/?",
        "https://www.nasa.gov/#",
        "https://www.nasa.gov/#section",
        "https://www.nasa.gov",
        "https://WWW.NASA.GOV/",
        " https://www.nasa.gov/",
        "https://www.nasa.gov/\n",
        "https://www.nasa.gov/a b",
        "https://www.nasa.gov/a/../terms",
        "https://www.nasa.gov\\terms",
        "https://www.%6easa.gov/",
        "https://127.0.0.1/",
        "https://[::1]/",
        "/terms",
    ] {
        assert!(plan.validate_destination_syntax(raw).is_err(), "{raw}");
    }
    assert!(plan
        .validate_destination_syntax("https://www.nasa.gov/space%20science")
        .is_ok());
    assert!(plan
        .validate_destination_syntax(&format!("https://{}.nasa.gov/", "x".repeat(64)))
        .is_err());
    assert!(plan
        .validate_destination_syntax(&format!(
            "https://www.nasa.gov/{}",
            "x".repeat(MAX_URL_BYTES)
        ))
        .is_err());
}

#[test]
fn selected_access_urls_are_exact_distinct_bounded_and_not_content_authority() {
    let plan = ValidatedPublisherAccessPlan::validate(input()).unwrap();
    assert_eq!(
        plan.selected_access_url("https://www.nasa.gov/terms/")
            .unwrap()
            .host(),
        "www.nasa.gov"
    );
    assert!(plan
        .validate_destination_syntax("https://www.nasa.gov/other")
        .is_ok());
    assert!(plan
        .selected_access_url("https://www.nasa.gov/other")
        .is_err());
    let mut value = input();
    value.access_urls.push(value.access_urls[0].clone());
    assert!(ValidatedPublisherAccessPlan::validate(value).is_err());
    let mut value = input();
    value.access_urls = (0..MAX_ACCESS_URLS)
        .map(|i| format!("https://www.nasa.gov/access/{i}"))
        .collect();
    assert!(ValidatedPublisherAccessPlan::validate(value.clone()).is_ok());
    value.access_urls.push("https://www.nasa.gov/extra".into());
    assert!(ValidatedPublisherAccessPlan::validate(value).is_err());
    for raw in [
        "https://other.example/terms",
        "https://www.nasa.gov/terms?q=secret",
        "https://www.nasa.gov/terms#part",
    ] {
        let mut value = input();
        value.access_urls = vec![raw.into()];
        assert!(ValidatedPublisherAccessPlan::validate(value).is_err());
    }
}

#[test]
fn ceilings_zero_request_plan_and_only_tightening_are_distinct_from_execution() {
    let plan = ValidatedPublisherAccessPlan::validate(input()).unwrap();
    for limits in [
        ProfileLimits {
            max_hops: 3,
            max_requests: 50,
            max_seconds: 600,
        },
        ProfileLimits {
            max_hops: 2,
            max_requests: 51,
            max_seconds: 600,
        },
        ProfileLimits {
            max_hops: 2,
            max_requests: 50,
            max_seconds: 601,
        },
        ProfileLimits {
            max_hops: 2,
            max_requests: 50,
            max_seconds: 0,
        },
    ] {
        let mut value = input();
        value.effective_limits = limits;
        assert!(ValidatedPublisherAccessPlan::validate(value).is_err());
    }
    let reduced = plan
        .tighten(ProfileLimits {
            max_hops: 1,
            max_requests: 10,
            max_seconds: 30,
        })
        .unwrap();
    assert_ne!(reduced.plan_sha256(), plan.plan_sha256());
    assert_eq!(plan.input().effective_limits, input().effective_limits);
    for limits in [
        ProfileLimits {
            max_hops: 2,
            max_requests: 10,
            max_seconds: 30,
        },
        ProfileLimits {
            max_hops: 1,
            max_requests: 11,
            max_seconds: 30,
        },
        ProfileLimits {
            max_hops: 1,
            max_requests: 10,
            max_seconds: 31,
        },
    ] {
        assert!(reduced.tighten(limits).is_err());
    }
    let zero = reduced
        .tighten(ProfileLimits {
            max_hops: 0,
            max_requests: 0,
            max_seconds: 1,
        })
        .unwrap();
    assert_eq!(zero.input().effective_limits.max_requests, 0);
    assert_eq!(zero.tighten(zero.input().effective_limits).unwrap(), zero);
}

#[test]
fn plan_digest_binds_exact_order_session_and_query_without_sending_query_text() {
    let original = ValidatedPublisherAccessPlan::validate(input()).unwrap();
    // Independently encoded with Python json.dumps(compact, insertion order),
    // then hashlib.sha256: 549 exact UTF-8 bytes, no Rust payload helper.
    assert_eq!(
        original.plan_sha256(),
        "f2df6a8173a5cd439b053df0f462e55840f7f22569906669a7fd7d740f44a805"
    );
    let mut reversed = input();
    reversed.access_urls.reverse();
    assert_ne!(
        ValidatedPublisherAccessPlan::validate(reversed)
            .unwrap()
            .plan_sha256(),
        original.plan_sha256()
    );
    let mut session = input();
    session.session_id = "22222222-2222-4222-8222-222222222222".into();
    assert_ne!(
        ValidatedPublisherAccessPlan::validate(session)
            .unwrap()
            .plan_sha256(),
        original.plan_sha256()
    );
    let disclosure = serde_json::to_value(original.url_disclosure()).unwrap();
    assert_eq!(
        disclosure,
        serde_json::json!({"publisher_domain":"nasa.gov","includes_subdomains":true,
        "seed_url":"https://www.nasa.gov/missions/","access_urls":["https://www.nasa.gov/robots.txt","https://www.nasa.gov/terms/"],"local_query_sent":false})
    );
    assert!(!disclosure.to_string().contains("Artemis"));
    assert!(!disclosure
        .to_string()
        .contains(&original.input().local_query_sha256));
    assert!(!disclosure
        .to_string()
        .contains(&original.input().session_id));
    assert_eq!(sha256(BENCHMARK), BENCHMARK_SHA256);
}

#[test]
fn historical_v4_url_preview_and_native_gate_remain_unchanged() {
    let raw = "https://www.nasa.gov/missions/?q=local#section";
    let old = crate::policy::validate_https_url(raw).unwrap();
    assert_eq!(old.query(), Some("q=local"));
    assert_eq!(old.fragment(), None);
    let preview = crate::collection_api::preview(crate::collection_api::CollectionInput {
        urls: vec![raw.into()],
        max_hops: 2,
        max_requests: 50,
        max_seconds: 600,
    })
    .unwrap();
    assert_eq!(
        preview.collector_policy,
        "direct-https-durable-html-bounded-v4"
    );
    assert_eq!(preview.disclosure.followed_hosts, "selected_hosts_only");
    assert!(!std::hint::black_box(
        crate::collection_api::NATIVE_COLLECTION_ENABLED
    ));
    assert!(ValidatedPublisherAccessPlan::validate(input())
        .unwrap()
        .validate_destination_syntax(raw)
        .is_err());
}
