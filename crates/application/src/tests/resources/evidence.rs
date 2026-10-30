use super::*;

#[tokio::test]
async fn a_bounded_terminal_receipt_is_not_replayed_when_only_one_result_has_metadata() {
    let store = Arc::new(MeasuredUsage::default());
    let budget = measured_budget(store.clone());
    budget.debit("workers", "requests", 1.0).await.unwrap();
    store.state.lock().unwrap().missing_closing_meta = true;
    budget.flush_accounting().await.unwrap();
    assert_eq!(store.ledger(), store.real());
    budget.flush_accounting().await.unwrap();
    assert_eq!(store.state.lock().unwrap().flushes, 1);
    assert_eq!(
        store
            .accounting_usage()
            .iter()
            .map(|row| row.unmeasured_operations)
            .sum::<u64>(),
        1
    );
}

#[tokio::test]
async fn unresolved_final_commit_is_an_error_on_every_subsequent_flush() {
    let store = Arc::new(MeasuredUsage::default());
    let budget = measured_budget(store.clone());
    {
        let mut state = store.state.lock().unwrap();
        state.receipt((5, 7));
        state.fail_flush = true;
    }
    assert!(matches!(
        budget.flush_accounting().await,
        Err(DomainError::IndexUnavailable(_))
    ));
    assert!(matches!(
        budget.flush_accounting().await,
        Err(DomainError::IndexUnavailable(_))
    ));
    assert_eq!(store.state.lock().unwrap().flushes, 1);
    assert_eq!(store.ledger(), (0, 0));
    assert!(
        store
            .accounting_usage()
            .iter()
            .any(|row| row.unmeasured_operations > 0)
    );
}

#[tokio::test]
async fn unknown_receipts_from_yesterday_do_not_block_a_fresh_day() {
    let store = Arc::new(MeasuredUsage::default());
    let budget = measured_budget(store.clone());
    let yesterday = time::OffsetDateTime::now_utc()
        .date()
        .midnight()
        .assume_utc()
        .unix_timestamp()
        - 1;
    store.state.lock().unwrap().receipts.insert(
        yesterday,
        AccountingUsage {
            observed_at: yesterday,
            unmeasured_operations: 1,
            ..Default::default()
        },
    );
    budget.debit("workers", "requests", 1.0).await.unwrap();
    budget.flush_accounting().await.unwrap();
    let rows = store.usage().await.unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| row.reset_at.is_some_and(|end| end <= yesterday + 1))
            .count(),
        2
    );
    assert!(
        rows.iter()
            .filter(|row| row.reset_at.is_some_and(|end| end <= yesterday + 1))
            .all(|row| row.evidence == UsageEvidence::Unknown)
    );
    assert!(
        rows.iter()
            .filter(|row| row.reset_at.is_some_and(|end| end > yesterday + 1))
            .all(|row| row.evidence != UsageEvidence::Unknown)
    );
}

#[tokio::test]
async fn missing_measurements_block_independently_of_numeric_quota_policy() {
    let store = Arc::new(MeasuredUsage::default());
    let mut budget = measured_budget(store.clone());
    for rule in &mut budget.profile.rules {
        if rule.resource.service == "d1" {
            rule.policy.when_exhausted = Some(AdmissionAction::Observe);
            rule.policy.when_charge_unknown = Some(AdmissionAction::Block);
        }
    }
    budget
        .record_unmeasured(time::OffsetDateTime::now_utc().unix_timestamp())
        .await
        .unwrap();
    assert!(matches!(
        budget.debit("d1", "rows_read", 0.0).await,
        Err(DomainError::ResourcePolicy(_))
    ));
    let preview = budget
        .admission("workers", "requests", UsageEvidence::Bounded)
        .await;
    assert_eq!(preview.state, "blocked");
    assert_eq!(preview.reason.as_deref(), Some("usage_unknown"));
    for rule in &mut budget.profile.rules {
        rule.policy.when_charge_unknown = Some(AdmissionAction::Observe);
    }
    budget.debit("d1", "rows_read", 0.0).await.unwrap();
    budget.flush_accounting().await.unwrap();
    let rows = store.usage().await.unwrap();
    assert!(
        rows.iter()
            .all(|row| row.evidence == UsageEvidence::Unknown)
    );
}

#[tokio::test]
async fn a_lost_commit_response_never_replays_already_submitted_receipts() {
    for debit in [true, false] {
        let store = Arc::new(MeasuredUsage::default());
        let budget = measured_budget(store.clone());
        {
            let mut state = store.state.lock().unwrap();
            state.receipt((5, 7));
            state.fail_observation = true;
        }
        let result = if debit {
            budget.debit("workers", "requests", 1.0).await
        } else {
            budget.record(vec![]).await
        };
        assert!(matches!(result, Err(DomainError::IndexUnavailable(_))));
        budget.flush_accounting().await.unwrap();
        assert_eq!(store.ledger(), store.real());
        assert_eq!(
            store.state.lock().unwrap().consumed("requests"),
            if debit { 1.0 } else { 0.0 }
        );
        assert!(
            store
                .usage()
                .await
                .unwrap()
                .iter()
                .filter(|row| row.rule.starts_with("d1-"))
                .all(|row| row.evidence == UsageEvidence::Unknown)
        );
    }
}

#[tokio::test]
async fn timed_receipts_keep_their_day_and_closing_cost_is_charged_once() {
    let store = Arc::new(MeasuredUsage {
        fresh_closing: true,
        ..Default::default()
    });
    let budget = measured_budget(store.clone());
    let today = time::OffsetDateTime::now_utc()
        .date()
        .midnight()
        .assume_utc();
    let before = today - time::Duration::seconds(1);
    let after = today + time::Duration::seconds(1);
    let mut old_keys = Vec::new();
    for metric in ["rows_read", "rows_written"] {
        old_keys.push(
            budget
                .profile
                .charge(
                    "d1",
                    metric,
                    0.0,
                    &budget.account,
                    UsageEvidence::Exact,
                    before,
                )
                .unwrap(),
        );
    }
    {
        let mut state = store.state.lock().unwrap();
        state.charges.extend(old_keys.clone());
        state.receipt_at((7, 3), before.unix_timestamp());
        state.receipt_at((5, 2), after.unix_timestamp());
    }
    budget
        .close_accounting(after.unix_timestamp())
        .await
        .unwrap();
    {
        let state = store.state.lock().unwrap();
        for (key, expected) in old_keys.iter().zip([7.0, 3.0]) {
            assert_eq!(
                state
                    .charges
                    .iter()
                    .filter(|row| row.key == key.key)
                    .map(|row| row.amount)
                    .sum::<f64>(),
                expected
            );
        }
    }
    let real = store.real();
    assert_eq!(store.ledger(), (real.0 + 2, real.1));
    budget
        .close_accounting(after.unix_timestamp())
        .await
        .unwrap();
    assert_eq!(store.state.lock().unwrap().flushes, 1);
}

#[tokio::test]
async fn historical_failed_observation_is_not_replayed_in_the_new_day() {
    let store = Arc::new(MeasuredUsage::default());
    let budget = measured_budget(store.clone());
    let yesterday = time::OffsetDateTime::now_utc()
        .date()
        .midnight()
        .assume_utc()
        - time::Duration::seconds(1);
    {
        let mut state = store.state.lock().unwrap();
        state.receipt_at((5, 7), yesterday.unix_timestamp());
        state.fail_observation = true;
    }
    assert!(matches!(
        budget.debit("workers", "requests", 1.0).await,
        Err(DomainError::IndexUnavailable(_))
    ));
    budget.flush_accounting().await.unwrap();
    assert_eq!(store.ledger(), store.real());
    assert_eq!(store.state.lock().unwrap().consumed("requests"), 0.0);
}

#[tokio::test]
async fn expired_zero_row_guards_do_not_prevent_consuming_a_valid_provider_reservation() {
    let store = Arc::new(MeasuredUsage::default());
    let mut budget = measured_budget(store.clone());
    let at = time::OffsetDateTime::now_utc().unix_timestamp();
    budget.profile.context.billing_cycles.insert(
        "ai_search".into(),
        Some(UsageWindow {
            start: at - 3600,
            end: Some(at + 3600),
        }),
    );
    let mut reservation = budget
        .reserve("ai_search", "semantic_queries", 1.0, UsageEvidence::Bounded)
        .await
        .unwrap();
    for charge in &mut reservation.charges {
        if charge.amount == 0.0 {
            charge.window.end = Some(at - 1);
        }
    }
    budget
        .settle(&reservation, Settlement::Consume)
        .await
        .unwrap();
    assert_eq!(
        store.state.lock().unwrap().consumed("semantic_queries"),
        1.0
    );
}
