use super::*;

const NO_RECORD: i32 = -65554;

#[test]
fn negative_callbacks_never_invent_family_completion_or_poison_public_candidates() {
    for positive in ["1.1.1.1", "2606:4700:4700::1111"] {
        let mut answers = Answers::new();
        // The header declares all fields other than error undefined on error.
        // Fake private addresses/flags therefore must not be inspected or used.
        answers.record(u32::MAX, NO_RECORD, Some("127.0.0.1".parse().unwrap()));
        assert_eq!(answers.error, None);
        assert!(answers.values.is_empty());
        assert!(!answers.batch_ended);
        answers.record(2, 0, Some(positive.parse().unwrap()));
        answers.record(0, NO_RECORD, Some("::1".parse().unwrap()));
        assert_eq!(answers.error, None);
        assert_eq!(
            answers.values,
            vec![SocketAddr::new(positive.parse().unwrap(), 443)]
        );
        assert!(answers.batch_ended);
    }
}

#[test]
fn negative_callback_cannot_end_pending_batch_or_undo_a_removal() {
    let mut answers = Answers::new();
    let address = "1.1.1.1".parse().unwrap();
    answers.record(3, 0, Some(address));
    answers.record(0, NO_RECORD, None);
    assert_eq!(answers.error, None);
    assert!(!answers.batch_ended);
    assert_eq!(answers.values.len(), 1);
    answers.record(0, 0, Some(address));
    assert!(answers.values.is_empty());
    answers.record(2, NO_RECORD, Some(address));
    assert!(answers.values.is_empty());
    answers.record(2, 0, Some("8.8.8.8".parse().unwrap()));
    assert_eq!(answers.values, vec!["8.8.8.8:443".parse().unwrap()]);
}

#[test]
fn successful_private_add_or_removal_remains_fatal_after_negative_callback() {
    for flags in [0, 2] {
        let mut answers = Answers::new();
        answers.record(u32::MAX, NO_RECORD, None);
        answers.record(flags, 0, Some("127.0.0.1".parse().unwrap()));
        assert_eq!(answers.error, Some(StopReason::Policy));
    }
}

#[test]
fn negative_notifications_remain_subject_to_callback_limit() {
    let mut answers = Answers::new();
    for _ in 0..MAX_ADDRESSES * 4 {
        answers.record(u32::MAX, NO_RECORD, None);
    }
    assert_eq!(answers.error, None);
    answers.record(0, NO_RECORD, None);
    assert_eq!(answers.error, Some(StopReason::Policy));
}

#[test]
fn later_non_negative_failure_is_not_hidden_by_an_earlier_positive() {
    let mut answers = Answers::new();
    answers.record(2, 0, Some("1.1.1.1".parse().unwrap()));
    answers.record(0, -65563, None); // ServiceNotRunning, not NoSuchRecord.
    assert_eq!(answers.error, Some(StopReason::Network));
    assert_eq!(answers.ready(true), Err(StopReason::Network));
}

#[test]
fn stage_boundary_retains_only_current_public_candidates_without_completeness_claim() {
    let mut answers = Answers::new();
    let address = "1.1.1.1".parse().unwrap();
    answers.record(3, 0, Some(address));
    answers.record(0, NO_RECORD, None);
    assert_eq!(answers.ready(false), Ok(false));
    assert_eq!(answers.ready(true), Ok(true));
    answers.record(0, 0, Some(address));
    assert_eq!(answers.ready(false), Ok(false));
    assert_eq!(answers.ready(true), Err(StopReason::Network));
    answers.record(2, 0, Some("8.8.8.8".parse().unwrap()));
    assert_eq!(answers.ready(false), Ok(true));
    assert_eq!(answers.values, vec!["8.8.8.8:443".parse().unwrap()]);

    let answers = Answers::new();
    assert_eq!(answers.ready(false), Ok(false));
    assert_eq!(answers.ready(true), Err(StopReason::Timeout));
}

#[test]
fn every_overall_stop_wins_over_stage_expiry_and_retained_candidates() {
    let mut answers = Answers::new();
    answers.record(3, 0, Some("1.1.1.1".parse().unwrap()));
    answers.record(0, NO_RECORD, None);
    let stage_deadline = Instant::now();
    let now = wall_now();
    let window = || ExecutionWindow::new(now + 20_000, now, Duration::from_secs(20)).unwrap();
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert_eq!(
        ready_after_check(&answers, &mut window(), &cancellation, stage_deadline),
        Err(StopReason::Cancelled)
    );
    let mut expired = window();
    expired.monotonic_deadline = Instant::now();
    assert_eq!(
        ready_after_check(
            &answers,
            &mut expired,
            &CancellationToken::default(),
            stage_deadline
        ),
        Err(StopReason::Deadline)
    );
    let mut backward = window();
    backward.wall_now = || 0;
    assert_eq!(
        ready_after_check(
            &answers,
            &mut backward,
            &CancellationToken::default(),
            stage_deadline
        ),
        Err(StopReason::ClockChanged)
    );
}

#[test]
fn mixed_add_remove_order_and_address_bound_remain_exact() {
    let mut answers = Answers::new();
    let v4 = "1.1.1.1".parse().unwrap();
    let v6 = "2606:4700:4700::1111".parse().unwrap();
    answers.record(3, 0, Some(v4));
    answers.record(3, 0, Some(v6));
    answers.record(0, NO_RECORD, None);
    answers.record(0, 0, Some(v4));
    assert_eq!(answers.values, vec![SocketAddr::new(v6, 443)]);
    assert_eq!(answers.ready(false), Ok(true));

    let mut bounded = Answers::new();
    for last in 1..=MAX_ADDRESSES {
        bounded.record(3, 0, Some(IpAddr::V4(Ipv4Addr::new(1, 1, 1, last as u8))));
    }
    bounded.record(0, NO_RECORD, None);
    assert_eq!(bounded.ready(true), Ok(true));
    bounded.record(2, 0, Some("8.8.8.8".parse().unwrap()));
    assert_eq!(bounded.ready(true), Err(StopReason::Policy));
}
