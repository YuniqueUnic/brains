use super::*;

#[tokio::test]
async fn measured_query_sequence_counts_receipts_once_and_closes_with_one_bound() {
    let store = Arc::new(MeasuredUsage::default());
    let budget = measured_budget(store.clone());
    budget.debit("workers", "requests", 1.0).await.unwrap();
    for _ in 0..10 {
        budget.debit("d1", "rows_read", 0.0).await.unwrap();
        {
            let mut state = store.state.lock().unwrap();
            state.application.0 += 7;
            state.application.1 += 3;
        }
        budget.record(query_charges(&budget, 7, 3)).await.unwrap();
    }
    budget.flush_accounting().await.unwrap();
    let real = store.real();
    let ledger = store.ledger();
    assert_eq!(ledger, real);
    assert!(ledger.0 >= real.0 && ledger.0 <= 2 * real.0);
    assert!(ledger.1 >= real.1 && ledger.1 <= 2 * real.1);
    let state = store.state.lock().unwrap();
    assert_eq!(state.checks, 10);
    assert_eq!(state.flushes, 1);
    assert!(
        state
            .charges
            .iter()
            .filter(
                |c| c.evidence == UsageEvidence::Bounded && c.measure.metric.starts_with("rows_")
            )
            .count()
            == 2
    );
}

#[tokio::test]
async fn zero_amount_precheck_is_read_only_and_its_read_receipt_is_flushed() {
    let store = Arc::new(MeasuredUsage::default());
    let budget = measured_budget(store.clone());
    budget.debit("d1", "rows_written", 0.0).await.unwrap();
    assert_eq!(store.real(), (2, 0));
    assert_eq!(store.ledger(), (0, 0));
    budget.flush_accounting().await.unwrap();
    assert_eq!(store.ledger(), (6, 10));
    assert_eq!(store.real(), (4, 4));
}

#[tokio::test]
async fn first_closing_insert_is_covered_and_idle_flush_does_not_recur() {
    let store = Arc::new(MeasuredUsage {
        fresh_closing: true,
        ..Default::default()
    });
    store.state.lock().unwrap().receipt((1, 0));
    let budget = measured_budget(store.clone());
    budget.flush_accounting().await.unwrap();
    assert_eq!(store.ledger(), (5, 10));
    assert_eq!(store.real(), (3, 10));
    budget.flush_accounting().await.unwrap();
    assert_eq!(store.state.lock().unwrap().flushes, 1);
    assert_eq!(store.real(), (3, 10));
}

#[tokio::test]
async fn receipt_arriving_during_closing_remains_pending() {
    let store = Arc::new(MeasuredUsage::default());
    let budget = measured_budget(store.clone());
    {
        let mut state = store.state.lock().unwrap();
        state.receipt((1, 0));
        state.during_flush = (5, 3);
    }
    budget.flush_accounting().await.unwrap();
    budget.flush_accounting().await.unwrap();
    let real = store.real();
    assert_eq!(store.ledger(), (real.0 + 4 - 2, real.1 + 10 - 4));
    budget.flush_accounting().await.unwrap();
    assert_eq!(store.state.lock().unwrap().flushes, 2);
}

#[tokio::test]
async fn concurrent_completed_queries_do_not_duplicate_bookkeeping_receipts() {
    let store = Arc::new(MeasuredUsage::default());
    let budget = measured_budget(store.clone());
    let calls = (0..10).map(|_| {
        let charges = query_charges(&budget, 7, 3);
        {
            let mut state = store.state.lock().unwrap();
            state.application.0 += 7;
            state.application.1 += 3;
        }
        budget.record(charges)
    });
    for result in futures::future::join_all(calls).await {
        result.unwrap();
    }
    budget.flush_accounting().await.unwrap();
    let real = store.real();
    assert_eq!(store.ledger(), real);
}

#[tokio::test]
async fn rejected_request_keeps_its_measured_receipt_for_final_accounting() {
    let store = Arc::new(MeasuredUsage::default());
    let mut budget = measured_budget(store.clone());
    budget
        .profile
        .rules
        .iter_mut()
        .find(|r| r.id == "worker-requests")
        .unwrap()
        .policy
        .maximum = Some(0.0);
    assert!(matches!(
        budget.debit("workers", "requests", 1.0).await,
        Err(DomainError::QuotaExceeded { .. })
    ));
    assert_eq!(store.ledger(), (0, 0));
    budget.flush_accounting().await.unwrap();
    assert_eq!(store.ledger(), (6, 10));
    assert_eq!(store.state.lock().unwrap().consumed("requests"), 0.0);
}

#[tokio::test]
async fn released_provider_reservation_never_refunds_bookkeeping() {
    let store = Arc::new(MeasuredUsage::default());
    let mut budget = measured_budget(store.clone());
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    budget.profile.context.billing_cycles.insert(
        "ai_search".into(),
        Some(UsageWindow {
            start: now - 60,
            end: Some(now + 3600),
        }),
    );
    budget.debit("workers", "requests", 1.0).await.unwrap();
    let reservation = budget
        .reserve("ai_search", "semantic_queries", 1.0, UsageEvidence::Bounded)
        .await
        .unwrap();
    assert!(
        reservation
            .charges
            .iter()
            .filter(|c| c.measure.metric.starts_with("rows_"))
            .all(|c| c.amount == 0.0)
    );
    budget
        .settle(&reservation, Settlement::Release)
        .await
        .unwrap();
    budget.flush_accounting().await.unwrap();
    let real = store.real();
    assert_eq!(store.ledger(), real);
    assert_eq!(
        store.state.lock().unwrap().consumed("semantic_queries"),
        0.0
    );
}

#[tokio::test]
async fn reservation_rechecks_newly_measured_cost_before_provider_dispatch() {
    let store = Arc::new(MeasuredUsage::default());
    let mut budget = measured_budget(store.clone());
    store.state.lock().unwrap().receipt((0, 1));
    budget
        .profile
        .rules
        .iter_mut()
        .find(|r| r.id == "d1-writes")
        .unwrap()
        .policy
        .maximum = Some(2.0);
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    budget.profile.context.billing_cycles.insert(
        "ai_search".into(),
        Some(UsageWindow {
            start: now - 60,
            end: Some(now + 3600),
        }),
    );
    assert!(matches!(
        budget
            .reserve("ai_search", "semantic_queries", 1.0, UsageEvidence::Bounded)
            .await,
        Err(DomainError::QuotaExceeded { .. })
    ));
    assert!(store.state.lock().unwrap().reservations.is_empty());
    budget.flush_accounting().await.unwrap();
    assert!(store.ledger().1 >= store.real().1);
}

#[tokio::test]
async fn native_store_defaults_do_not_add_d1_or_closing_charges() {
    let store = Arc::new(RecordedUsage::default());
    assert_eq!(store.accounting_bounds(), (0, 0));
    assert!(store.accounting_usage().is_empty());
    let budget = ResourceBudget::new(
        ResourceProfile::parse(CLOUDFLARE_FREE_PROFILE).unwrap(),
        "native".into(),
        store.clone(),
    );
    budget.debit("workers", "requests", 1.0).await.unwrap();
    budget.flush_accounting().await.unwrap();
    let observed = store.observations.lock().unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].measure.metric, "requests");
}
