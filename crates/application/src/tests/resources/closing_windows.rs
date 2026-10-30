use super::*;

#[tokio::test]
async fn closing_cost_requires_both_exact_window_keys() {
    let store = Arc::new(MeasuredUsage {
        fresh_closing: true,
        ..Default::default()
    });
    let mut budget = measured_budget(store.clone());
    budget.debit("workers", "requests", 1.0).await.unwrap();
    let existing = query_charges(&budget, 0, 0);
    assert_eq!(store.accounting_bounds_for(&existing), (2, 2));
    for index in 0..2 {
        let mut mixed = existing.clone();
        mixed[index].key.push_str("-new-window");
        assert_eq!(store.accounting_bounds_for(&mixed), (4, 10));
    }

    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    budget.profile.context.billing_cycles.insert(
        "new-d1-window".into(),
        Some(UsageWindow {
            start: now - 1,
            end: Some(now + 3600),
        }),
    );
    for rule in &mut budget.profile.rules {
        if rule.resource.service == "d1"
            && matches!(rule.measure.metric.as_str(), "rows_read" | "rows_written")
        {
            rule.window = Window::BillingCycle {
                r#ref: "new-d1-window".into(),
            };
        }
    }
    let new_window = query_charges(&budget, 0, 0);
    assert!(
        existing
            .iter()
            .zip(&new_window)
            .all(|(old, new)| old.key != new.key)
    );
    assert_eq!(store.accounting_bounds_for(&new_window), (4, 10));
    budget.flush_accounting().await.unwrap();
    let real = store.real();
    assert_eq!(store.ledger(), (real.0 + 2, real.1));
    assert_eq!(store.state.lock().unwrap().flushes, 1);
}
