use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use brains_domain::{DomainError, DomainResult, resources::*};
use brains_ports::resources::{AccountingUsage, UsageStore};
use futures::future::BoxFuture;

use crate::resources::ResourceBudget;

mod closing_windows;
mod evidence;
mod scenarios;

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
    receipts: BTreeMap<i64, AccountingUsage>,
    application: (u64, u64),
    charges: Vec<BudgetCharge>,
    reservations: Vec<Reservation>,
    terminal: Vec<String>,
    checks: usize,
    flushes: usize,
    during_flush: (u64, u64),
    fail_observation: bool,
    fail_flush: bool,
    missing_closing_meta: bool,
}

impl MeasuredState {
    fn receipt(&mut self, cost: (u64, u64)) {
        self.receipt_at(cost, time::OffsetDateTime::now_utc().unix_timestamp());
    }

    fn receipt_at(&mut self, cost: (u64, u64), observed_at: i64) {
        self.accounting.0 += cost.0;
        self.accounting.1 += cost.1;
        let receipt = self.receipts.entry(observed_at).or_insert(AccountingUsage {
            observed_at,
            ..Default::default()
        });
        receipt.rows_read += cost.0;
        receipt.rows_written += cost.1;
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
            if charge.block_unknown
                && (charge.evidence == UsageEvidence::Unknown
                    || self
                        .charges
                        .iter()
                        .any(|row| row.key == charge.key && row.evidence == UsageEvidence::Unknown))
            {
                return Err(DomainError::ResourcePolicy("usage is unknown".into()));
            }
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
    fn accounting_usage(&self) -> Vec<AccountingUsage> {
        self.state
            .lock()
            .unwrap()
            .receipts
            .values()
            .copied()
            .collect()
    }

    fn accounting_bounds_for(&self, charges: &[BudgetCharge]) -> (u64, u64) {
        let state = self.state.lock().unwrap();
        charges.iter().fold((0, 0), |total, charge| {
            let cost = if state.charges.iter().any(|row| row.key == charge.key) {
                (1, 1)
            } else {
                (2, 5)
            };
            (total.0 + cost.0, total.1 + cost.1)
        })
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
            if std::mem::take(&mut state.fail_observation) {
                let at = time::OffsetDateTime::now_utc().unix_timestamp();
                state.receipts.get_mut(&at).unwrap().unmeasured_operations += 1;
                return Err(DomainError::IndexUnavailable(
                    "response lost after commit".into(),
                ));
            }
            Ok(())
        })
    }

    fn observe<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(async move {
            tokio::task::yield_now().await;
            let mut state = self.state.lock().unwrap();
            state.charges.extend_from_slice(charges);
            state.receipt((2, 3));
            if std::mem::take(&mut state.fail_observation) {
                let at = time::OffsetDateTime::now_utc().unix_timestamp();
                state.receipts.get_mut(&at).unwrap().unmeasured_operations += 1;
                return Err(DomainError::IndexUnavailable(
                    "response lost after commit".into(),
                ));
            }
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
        observed_at: i64,
    ) -> BoxFuture<'a, DomainResult<AccountingUsage>> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            if std::mem::take(&mut state.fail_flush) {
                state.flushes += 1;
                state
                    .receipts
                    .entry(observed_at)
                    .or_insert(AccountingUsage {
                        observed_at,
                        ..Default::default()
                    })
                    .unmeasured_operations += 1;
                return Err(DomainError::IndexUnavailable(
                    "closing commit unresolved".into(),
                ));
            }
            let cost = charges.iter().fold((0, 0), |total, charge| {
                let cost = if state.charges.iter().any(|row| row.key == charge.key) {
                    (1, 1)
                } else {
                    (1, if self.fresh_closing { 5 } else { 2 })
                };
                (total.0 + cost.0, total.1 + cost.1)
            });
            assert!(
                charges
                    .iter()
                    .all(|c| matches!(c.evidence, UsageEvidence::Bounded | UsageEvidence::Unknown))
            );
            state.charges.extend_from_slice(charges);
            let missing = std::mem::take(&mut state.missing_closing_meta);
            let measured = if missing {
                (cost.0 - 1, cost.1 - 1)
            } else {
                cost
            };
            state.receipt_at(measured, observed_at);
            if missing {
                state.accounting.0 += 1;
                state.accounting.1 += 1;
                state
                    .receipts
                    .get_mut(&observed_at)
                    .unwrap()
                    .unmeasured_operations += 1;
            }
            let concurrent = std::mem::take(&mut state.during_flush);
            state.receipt(concurrent);
            state.flushes += 1;
            Ok(AccountingUsage {
                observed_at,
                rows_read: measured.0,
                rows_written: measured.1,
                unmeasured_operations: u64::from(missing),
            })
        })
    }

    fn usage(&self) -> BoxFuture<'_, DomainResult<Vec<BudgetUsage>>> {
        Box::pin(async {
            let state = self.state.lock().unwrap();
            let mut rows = BTreeMap::<String, BudgetUsage>::new();
            for charge in &state.charges {
                let row = rows.entry(charge.key.clone()).or_insert(BudgetUsage {
                    key: charge.key.clone(),
                    meter: charge.meter.clone(),
                    rule: charge.rule.clone(),
                    unit: charge.measure.unit.clone(),
                    consumed: 0.0,
                    reserved: 0.0,
                    evidence: charge.evidence,
                    limit: charge.limit,
                    reset_at: charge.window.end,
                    observed_at: None,
                    expires_at: None,
                    source: None,
                });
                row.consumed += charge.amount;
                row.evidence = row.evidence.max(charge.evidence);
            }
            Ok(rows.into_values().collect())
        })
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
