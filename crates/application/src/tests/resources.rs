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
    fn accounting_bounds(&self) -> (u64, u64) {
        (64, 32)
    }

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
        Arc::new(RecordedUsage::default()),
    );
    assert!(matches!(
        budget.debit("workers", "requests", 1.0).await,
        Err(DomainError::ResourcePolicy(_))
    ));
}
