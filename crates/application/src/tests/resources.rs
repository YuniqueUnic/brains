use std::sync::{Arc, Mutex};

use brains_domain::{DomainError, DomainResult, resources::*};
use brains_ports::resources::UsageStore;
use futures::future::BoxFuture;

use crate::resources::ResourceBudget;

#[derive(Default)]
struct RecordedUsage {
    observations: Mutex<Vec<BudgetCharge>>,
    reservations: Mutex<Vec<Reservation>>,
    settlements: Mutex<Vec<(String, Settlement)>>,
}

impl UsageStore for RecordedUsage {
    fn debit<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        self.observe(charges)
    }

    fn observe<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            self.observations.lock().unwrap().extend_from_slice(charges);
            Ok(())
        })
    }

    fn reserve<'a>(&'a self, reservation: &'a Reservation) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            self.reservations.lock().unwrap().push(reservation.clone());
            Ok(())
        })
    }

    fn settle<'a>(&'a self, id: &'a str, outcome: Settlement) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            self.settlements.lock().unwrap().push((id.into(), outcome));
            Ok(())
        })
    }

    fn usage(&self) -> BoxFuture<'_, DomainResult<Vec<BudgetUsage>>> {
        Box::pin(async { Ok(vec![]) })
    }
}

fn profile_with_unknown_d1_period() -> ResourceProfile {
    let mut profile = ResourceProfile::parse(CLOUDFLARE_FREE_PROFILE).unwrap();
    for rule in &mut profile.rules {
        if rule.resource.service == "d1" && rule.scope.kind == ScopeKind::Account {
            rule.window = Window::BillingCycle {
                r#ref: "unconfigured-d1-period".into(),
            };
        }
        if rule.id == "worker-requests" {
            rule.policy.maximum = Some(0.0);
        }
    }
    profile
}

#[tokio::test]
async fn observation_keeps_measurable_usage_without_blocking_on_unknown_periods() {
    let store = Arc::new(RecordedUsage::default());
    let budget = ResourceBudget::new(
        profile_with_unknown_d1_period(),
        "test".into(),
        store.clone(),
    )
    .with_enforcement(false);
    budget.debit("workers", "requests", 1.0).await.unwrap();
    let observed = store.observations.lock().unwrap().clone();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].rule, "worker-requests");
    assert_eq!(observed[0].amount, 1.0);
    assert_eq!(observed[0].limit, 0.0);
    assert_eq!(observed[0].action, AdmissionAction::Observe);

    let d1_result = budget.profile.charge(
        "d1",
        "rows_read",
        3.0,
        &budget.account,
        UsageEvidence::Exact,
        time::OffsetDateTime::now_utc(),
    );
    assert!(budget.measured_charge(d1_result).unwrap().is_none());
    budget.record(vec![]).await.unwrap();
    assert_eq!(store.observations.lock().unwrap().len(), 1);

    let attempt = budget
        .reserve("workers_ai", "compute", 0.0, UsageEvidence::Unknown)
        .await
        .unwrap();
    assert_eq!(attempt.charges.len(), 1);
    assert_eq!(attempt.charges[0].evidence, UsageEvidence::Unknown);
    assert_eq!(attempt.charges[0].action, AdmissionAction::Observe);
    budget.settle(&attempt, Settlement::Consume).await.unwrap();
    assert_eq!(store.reservations.lock().unwrap().len(), 1);
    assert_eq!(
        store.settlements.lock().unwrap().as_slice(),
        &[(attempt.attempt_id, Settlement::Consume)]
    );
    assert_eq!(
        budget
            .admission("workers_ai", "compute", UsageEvidence::Unknown)
            .await
            .state,
        "observe"
    );
}

#[tokio::test]
async fn enforcing_a_budget_requires_its_accounting_periods() {
    let budget = ResourceBudget::new(
        profile_with_unknown_d1_period(),
        "test".into(),
        Arc::new(MeasuredUsage::default()),
    );
    assert!(matches!(
        budget.debit("workers", "requests", 1.0).await,
        Err(DomainError::ResourcePolicy(_))
    ));
}

#[derive(Default)]
struct MeasuredState {
    accounting: (u64, u64),
    application: (u64, u64),
    charges: Vec<BudgetCharge>,
    reservations: Vec<Reservation>,
    terminal: Vec<String>,
    checks: usize,
    flushes: usize,
    during_flush: (u64, u64),
}

impl MeasuredState {
    fn receipt(&mut self, cost: (u64, u64)) {
        self.accounting.0 += cost.0;
        self.accounting.1 += cost.1;
    }

    fn consumed(&self, metric: &str) -> f64 {
        self.charges
            .iter()
            .filter(|c| c.measure.metric == metric)
            .map(|c| c.amount)
            .sum()
    }

    fn admit(&self, charges: &[BudgetCharge]) -> DomainResult<()> {
        for charge in charges {
            let consumed: f64 = self
                .charges
                .iter()
                .filter(|c| c.key == charge.key)
                .map(|c| c.amount)
                .sum();
            let held: f64 = self
                .reservations
                .iter()
                .filter(|r| !self.terminal.contains(&r.attempt_id))
                .flat_map(|r| &r.charges)
                .filter(|c| c.meter == charge.meter)
                .map(|c| c.amount)
                .sum();
            if charge.action == AdmissionAction::Block
                && consumed + held + charge.amount > charge.limit
            {
                return Err(DomainError::QuotaExceeded {
                    rule: charge.rule.clone(),
                    reset_at: charge.window.end,
                });
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct MeasuredUsage {
    state: Mutex<MeasuredState>,
    fresh_closing: bool,
}

impl MeasuredUsage {
    fn real(&self) -> (u64, u64) {
        let state = self.state.lock().unwrap();
        (
            state.accounting.0 + state.application.0,
            state.accounting.1 + state.application.1,
        )
    }

    fn ledger(&self) -> (u64, u64) {
        let state = self.state.lock().unwrap();
        (
            state.consumed("rows_read") as u64,
            state.consumed("rows_written") as u64,
        )
    }
}

impl UsageStore for MeasuredUsage {
    fn accounting_bounds(&self) -> (u64, u64) {
        (4, 10)
    }
    fn accounting_usage(&self) -> (u64, u64) {
        self.state.lock().unwrap().accounting
    }

    fn check<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.receipt((2, 0));
            state.checks += 1;
            state.admit(charges)
        })
    }

    fn debit<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            tokio::task::yield_now().await;
            let mut state = self.state.lock().unwrap();
            if let Err(error) = state.admit(charges) {
                state.receipt((2, 0));
                return Err(error);
            }
            state.charges.extend_from_slice(charges);
            state.receipt((2, 3));
            Ok(())
        })
    }

    fn observe<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            tokio::task::yield_now().await;
            let mut state = self.state.lock().unwrap();
            state.charges.extend_from_slice(charges);
            state.receipt((2, 3));
            Ok(())
        })
    }

    fn reserve<'a>(&'a self, reservation: &'a Reservation) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.receipt((7, 6));
            state.admit(&reservation.charges)?;
            state.reservations.push(reservation.clone());
            Ok(())
        })
    }

    fn settle<'a>(&'a self, id: &'a str, outcome: Settlement) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.receipt((3, 2));
            if matches!(outcome, Settlement::Uncertain)
                || state.terminal.iter().any(|done| done == id)
            {
                return Ok(());
            }
            if matches!(outcome, Settlement::Consume) {
                let charges = state
                    .reservations
                    .iter()
                    .find(|r| r.attempt_id == id)
                    .unwrap()
                    .charges
                    .clone();
                state.charges.extend(charges);
            }
            state.terminal.push(id.into());
            Ok(())
        })
    }

    fn flush_accounting<'a>(
        &'a self,
        charges: &'a [BudgetCharge],
    ) -> BoxFuture<'a, DomainResult<(u64, u64)>> {
        Box::pin(async move {
            let cost = (2, if self.fresh_closing { 10 } else { 4 });
            let mut state = self.state.lock().unwrap();
            assert_eq!(charges.len(), 2);
            assert!(charges.iter().all(|c| c.evidence == UsageEvidence::Bounded));
            assert_eq!(charges[0].window, charges[1].window);
            state.charges.extend_from_slice(charges);
            state.receipt(cost);
            let concurrent = std::mem::take(&mut state.during_flush);
            state.receipt(concurrent);
            state.flushes += 1;
            Ok(cost)
        })
    }

    fn usage(&self) -> BoxFuture<'_, DomainResult<Vec<BudgetUsage>>> {
        Box::pin(async { Ok(vec![]) })
    }
}

fn measured_budget(store: Arc<MeasuredUsage>) -> ResourceBudget {
    ResourceBudget::new(
        ResourceProfile::parse(CLOUDFLARE_FREE_PROFILE).unwrap(),
        "test".into(),
        store,
    )
}

fn query_charges(budget: &ResourceBudget, reads: u64, writes: u64) -> Vec<BudgetCharge> {
    [("rows_read", reads), ("rows_written", writes)]
        .into_iter()
        .map(|(metric, amount)| {
            budget
                .profile
                .charge(
                    "d1",
                    metric,
                    amount as f64,
                    &budget.account,
                    UsageEvidence::Exact,
                    time::OffsetDateTime::now_utc(),
                )
                .unwrap()
        })
        .collect()
}

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
    assert_eq!(ledger, (real.0 + 4 - 2, real.1 + 10 - 4));
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
    assert_eq!(
        store.ledger(),
        (real.0 + 2 * (4 - 2), real.1 + 2 * (10 - 4))
    );
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
    assert_eq!(store.ledger(), (real.0 + 4 - 2, real.1 + 10 - 4));
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
    assert_eq!(store.ledger(), (real.0 + 4 - 2, real.1 + 10 - 4));
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
    assert_eq!(store.accounting_usage(), (0, 0));
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
