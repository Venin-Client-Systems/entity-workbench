use super::*;

#[test]
fn passive_observer_attaches_once_without_runtime_execution_or_replacement() {
    let runtime = VerifiedGraphRuntime {
        prefix: PathBuf::from("not-an-executable-test-prefix"),
        observation: std::sync::OnceLock::new(),
    };
    let first = std::sync::Arc::new(TestObservation::new(false));
    runtime.attach_observation(first.clone()).unwrap();
    assert!(runtime
        .attach_observation(std::sync::Arc::new(TestObservation::new(false)))
        .is_err());
    assert!(std::sync::Arc::ptr_eq(
        runtime.observation.get().unwrap(),
        &first
    ));
    assert_eq!(first.receipt()["execution_calls"], 0);
    assert_eq!(first.receipt()["launch_count"], 0);
    let second = std::sync::Arc::new(TestObservation::new(false));
    let consuming = VerifiedGraphRuntime {
        prefix: PathBuf::from("not-an-executable-test-prefix"),
        observation: std::sync::OnceLock::new(),
    }
    .observe(second.clone());
    assert!(std::sync::Arc::ptr_eq(
        consuming.observation.get().unwrap(),
        &second
    ));
}

fn request() -> Vec<u8> {
    let fixtures: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../../workers/python/fixtures/canonical-graph-cases.v1.json"
    ))
    .unwrap();
    serde_json::to_vec(&fixtures["cases"][0]["request"]).unwrap()
}
fn receipt(binding: &Binding, raw: &[u8]) -> serde_json::Value {
    serde_json::json!({"schema_version":1,"recipe":RECIPE,"job_id":binding.job_id,
        "manifest_sha256":MANIFEST,"python_version":"3.13.15","isolated":true,"no_site":true,
        "no_bytecode":true,"verified_paths":true,"checks":{
            "versions":serde_json::from_slice::<serde_json::Value>(VERSIONS).unwrap(),
            "imported_modules":["networkx"],"backend_metadata_checked":true,
            "capture_nonce":binding.nonce,"request_identity":binding.request,
            "result_identity":Identity::of(raw)}})
}

#[test]
fn app_binding_accepts_over_legacy_limit_and_preserves_exact_result_bytes() {
    let mut request = request();
    request.resize(64 * 1024 + 1, b' ');
    let binding = Binding::new(&request).unwrap();
    assert_ne!(binding.job_id, Binding::new(&request).unwrap().job_id);
    // The adapter preserves even malformed graph bytes for the store authority;
    // it cannot turn duplicate keys into an accepted canonical record itself.
    let raw = b"{\"duplicate\":1,\"duplicate\":2}\n".to_vec();
    let good = serde_json::to_vec(&receipt(&binding, &raw)).unwrap();
    assert_eq!(binding.accept(&good, raw.clone()).unwrap(), raw);
    request.resize(REQUEST_LIMIT + 1, b' ');
    assert!(Binding::new(&request).is_err());
}

#[test]
fn wrapper_requires_every_runtime_launch_capture_and_raw_byte_binding() {
    let binding = Binding::new(&request()).unwrap();
    let raw = b"fixed raw graph result".to_vec();
    let good = receipt(&binding, &raw);
    let mutations = [
        (
            "/job_id",
            serde_json::json!(uuid::Uuid::new_v4().to_string()),
        ),
        ("/recipe", serde_json::json!("python-canonical-graph-v1")),
        ("/manifest_sha256", serde_json::json!("0".repeat(64))),
        ("/python_version", serde_json::json!("3.13.14")),
        ("/isolated", serde_json::json!(false)),
        ("/no_site", serde_json::json!(false)),
        ("/no_bytecode", serde_json::json!(false)),
        ("/verified_paths", serde_json::json!(false)),
        (
            "/checks/capture_nonce",
            serde_json::json!(uuid::Uuid::new_v4().to_string()),
        ),
        ("/checks/backend_metadata_checked", serde_json::json!(false)),
        ("/checks/request_identity/bytes", serde_json::json!(0)),
        (
            "/checks/request_identity/sha256",
            serde_json::json!("0".repeat(64)),
        ),
        (
            "/checks/result_identity/sha256",
            serde_json::json!("0".repeat(64)),
        ),
        ("/checks/versions/networkx", serde_json::json!("other")),
        ("/checks/imported_modules", serde_json::json!(["other"])),
    ];
    for (pointer, replacement) in mutations {
        let mut bad = good.clone();
        *bad.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            matches!(
                binding.accept(&serde_json::to_vec(&bad).unwrap(), raw.clone()),
                Err(Error::InvalidWorkerResult(_))
            ),
            "{pointer}"
        );
    }
    let bytes = serde_json::to_vec(&good).unwrap();
    let mut changed = raw.clone();
    changed.push(b' ');
    assert!(binding.accept(&bytes, changed).is_err());
    assert!(binding
        .accept(&bytes, vec![b'x'; RESULT_LIMIT + 1])
        .is_err());
    assert!(binding.accept(&vec![b' '; WRAPPER_LIMIT + 1], raw).is_err());
}

#[test]
fn duplicates_unknown_fields_and_noncanonical_nonce_cannot_pass_receipt_binding() {
    let binding = Binding::new(&request()).unwrap();
    let raw = b"fixed".to_vec();
    let good = receipt(&binding, &raw);
    let text = serde_json::to_string(&good).unwrap();
    for (from, to) in [
        (
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
        ),
        (
            "\"networkx\":\"3.6.1\"",
            "\"networkx\":\"bad\",\"networkx\":\"3.6.1\"",
        ),
    ] {
        let bad = text.replacen(from, to, 1);
        assert_ne!(bad, text);
        assert!(binding.accept(bad.as_bytes(), raw.clone()).is_err());
    }
    let mut bad = good;
    bad["checks"]["campaign_id"] = serde_json::json!("not-an-app-field");
    assert!(binding
        .accept(&serde_json::to_vec(&bad).unwrap(), raw)
        .is_err());
    let mut req: serde_json::Value = serde_json::from_slice(&request()).unwrap();
    for nonce in [
        "bad".to_owned(),
        uuid::Uuid::new_v4().simple().to_string(),
        uuid::Uuid::nil().to_string(),
    ] {
        req["nonce"] = nonce.into();
        assert!(Binding::new(&serde_json::to_vec(&req).unwrap()).is_err());
    }
}

#[test]
fn configuration_never_discovers_or_executes_a_host_runtime() {
    let temp = tempfile::tempdir().unwrap();
    assert!(matches!(
        VerifiedGraphRuntime::from_app_engines(temp.path(), &CancellationToken::default()),
        Err(Error::Blocked(_))
    ));
    // The fixed version-only asset is identical to the previously pinned set.
    let probe: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../../workers/python/probe/fixture.json"
    ))
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(VERSIONS).unwrap(),
        probe["versions"]
    );
}
