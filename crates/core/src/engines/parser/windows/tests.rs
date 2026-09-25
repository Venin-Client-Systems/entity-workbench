use super::*;
use workbench_windows_worker::ResourceLimit;

fn output(job: &java::Job, original: &[u8]) -> java::JavaOutput {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "protocol_version":1,"job_id":job.id().to_string(),
        "content_sha256":format!("{:x}",Sha256::digest(original)),
        "source_bytes":original.len(),"parser":"utf8-v1","media_type":"text/plain",
        "status":"complete","text":"Synthetic Windows adapter source",
        "metadata":{},"limitations":["no_source_anchors"],"error":null
    }))
    .unwrap();
    java::JavaOutput {
        job_id: job.id(),
        output_sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes,
        index: None,
    }
}

#[test]
fn output_must_match_both_request_identities_original_and_raw_digest() {
    let original = b"Synthetic Windows adapter source";
    let job = java::Job::parse(original.to_vec()).unwrap();
    assert!(accept(&job, original, output(&job, original)).is_ok());
    let mut transport = output(&job, original);
    transport.job_id = uuid::Uuid::new_v4();
    assert!(matches!(
        accept(&job, original, transport),
        Err(Error::InvalidWorkerResult(_))
    ));
    let mut corrupt = output(&job, original);
    corrupt.output_sha256 = "0".repeat(64);
    assert!(matches!(
        accept(&job, original, corrupt),
        Err(Error::InvalidWorkerResult(_))
    ));
    for (field, value) in [
        (
            "job_id",
            serde_json::json!(uuid::Uuid::new_v4().to_string()),
        ),
        ("content_sha256", serde_json::json!("0".repeat(64))),
        ("source_bytes", serde_json::json!(original.len() + 1)),
        ("extra", serde_json::json!(true)),
    ] {
        let mut altered = output(&job, original);
        let mut body: serde_json::Value = serde_json::from_slice(&altered.bytes).unwrap();
        body[field] = value;
        altered.bytes = serde_json::to_vec(&body).unwrap();
        altered.output_sha256 = format!("{:x}", Sha256::digest(&altered.bytes));
        assert!(
            matches!(
                accept(&job, original, altered),
                Err(Error::InvalidWorkerResult(_))
            ),
            "{field}"
        );
    }
    // Decode the actual bytes: generic JSON maps would hide these equivalent keys.
    let mut duplicate = output(&job, original);
    let raw = String::from_utf8(duplicate.bytes).unwrap().replace(
        "\"metadata\":{}",
        r#""metadata":{"Title":["first"],"\u0054itle":["last"]}"#,
    );
    duplicate.bytes = raw.into_bytes();
    duplicate.output_sha256 = format!("{:x}", Sha256::digest(&duplicate.bytes));
    assert!(matches!(
        accept(&job, original, duplicate),
        Err(Error::InvalidWorkerResult(_))
    ));
    for suffix in [br#", "protocol_version":1}"#.as_slice(), b"}{}"] {
        let mut altered = output(&job, original);
        altered.bytes.pop();
        altered.bytes.extend_from_slice(suffix);
        altered.output_sha256 = format!("{:x}", Sha256::digest(&altered.bytes));
        assert!(matches!(
            accept(&job, original, altered),
            Err(Error::InvalidWorkerResult(_))
        ));
    }
}

#[test]
fn fixed_recipe_observes_cancellation_without_demoting_exit_uncertainty() {
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        parse(
            Path::new("missing-runtime"),
            Path::new("missing-scratch"),
            b"synthetic",
            &token
        ),
        Err(Error::Interrupted(_))
    ));
    let token = CancellationToken::default();
    let original = b"Synthetic Windows adapter source";
    let result = execute_with(
        Path::new("selected-runtime"),
        Path::new("selected-scratch"),
        original,
        &token,
        |runtime, scratch, job, cancel| {
            assert_eq!(runtime, Path::new("selected-runtime").join("parser"));
            assert_eq!(scratch, Path::new("selected-scratch"));
            assert_eq!(job.role(), java::Role::Parser);
            cancel.cancel();
            Ok(output(job, original))
        },
    );
    assert!(matches!(result, Err(Error::Interrupted(_))));
    let token = CancellationToken::default();
    let result = execute_with(
        Path::new("selected-runtime"),
        Path::new("selected-scratch"),
        original,
        &token,
        |_, _, _, cancel| {
            cancel.cancel();
            Err(WorkerError::TerminationUnverified {
                cause: Box::new(WorkerError::Api {
                    operation: "SyntheticStop",
                    code: 5,
                }),
                prior: Some(Box::new(WorkerError::Cancelled)),
            })
        },
    );
    assert!(matches!(result, Err(Error::TerminationUnverified(_))));
}

#[test]
fn typed_worker_outcomes_keep_cleanup_limits_and_invalid_results_distinct() {
    assert!(matches!(
        classify(WorkerError::Cleanup {
            prior: Some(Box::new(WorkerError::Cancelled))
        }),
        Error::Cleanup(_)
    ));
    assert!(matches!(
        classify(WorkerError::ResourceLimit(ResourceLimit::OutputBytes)),
        Error::QuotaExhausted(_)
    ));
    assert!(matches!(
        classify(WorkerError::InvalidResult("synthetic malformed output")),
        Error::InvalidWorkerResult(_)
    ));
    assert!(matches!(
        classify(WorkerError::Blocked("missing verified runtime")),
        Error::Blocked(_)
    ));
    assert!(matches!(
        classify(WorkerError::Exit(0xC0000017)),
        Error::Validation(_)
    ));
    assert!(matches!(
        classify(WorkerError::Api {
            operation: "SyntheticApi",
            code: 5
        }),
        Error::Validation(_)
    ));
}
