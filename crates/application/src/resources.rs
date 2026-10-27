//! Budget orchestration: provider facts resolve a charge; a store admits it.

use std::sync::Arc;

use brains_domain::{DomainResult, HexId, resources::*};
use brains_ports::resources::UsageStore;

pub struct ResourceBudget {
    pub profile: ResourceProfile,
    pub account: String,
    store: Arc<dyn UsageStore>,
}

impl ResourceBudget {
    pub fn new(profile: ResourceProfile, account: String, store: Arc<dyn UsageStore>) -> Self {
        Self {
            profile,
            account,
            store,
        }
    }

    pub async fn reserve(
        &self,
        service: &str,
        metric: &str,
        amount: f64,
        evidence: UsageEvidence,
    ) -> DomainResult<Reservation> {
        let charges = self.charges(service, metric, amount, evidence)?;
        let reservation = Reservation {
            attempt_id: HexId::generate().as_str().to_owned(),
            charges,
        };
        self.store.reserve(&reservation).await?;
        Ok(reservation)
    }

    fn charges(
        &self,
        service: &str,
        metric: &str,
        amount: f64,
        evidence: UsageEvidence,
    ) -> DomainResult<Vec<BudgetCharge>> {
        let now = time::OffsetDateTime::now_utc();
        let charges =
            vec![
                self.profile
                    .charge(service, metric, amount, &self.account, evidence, now)?,
            ];
        self.with_accounting(charges, now)
    }

    /// Debit a known attempt before dispatch; a cancelled dispatch overcounts safely.
    pub async fn debit(&self, service: &str, metric: &str, amount: f64) -> DomainResult<()> {
        self.store
            .debit(&self.charges(service, metric, amount, UsageEvidence::Bounded)?)
            .await
    }

    /// Record completed I/O together; ledger accounting is added only once.
    pub async fn record(&self, charges: Vec<BudgetCharge>) -> DomainResult<()> {
        let charges = self.with_accounting(charges, time::OffsetDateTime::now_utc())?;
        self.store.observe(&charges).await
    }

    fn with_accounting(
        &self,
        mut charges: Vec<BudgetCharge>,
        now: time::OffsetDateTime,
    ) -> DomainResult<Vec<BudgetCharge>> {
        let (reads, writes) = self.store.accounting_bounds();
        for (metric, amount) in [("rows_read", reads), ("rows_written", writes)] {
            if amount > 0 {
                charges.push(self.profile.charge(
                    "d1",
                    metric,
                    amount as f64,
                    &self.account,
                    UsageEvidence::Bounded,
                    now,
                )?);
            }
        }
        let mut merged = std::collections::BTreeMap::<String, BudgetCharge>::new();
        for charge in charges {
            if let Some(existing) = merged.get_mut(&charge.key) {
                existing.amount += charge.amount;
                existing.evidence = existing.evidence.max(charge.evidence);
            } else {
                merged.insert(charge.key.clone(), charge);
            }
        }
        Ok(merged.into_values().collect())
    }

    pub async fn check_capacity(
        &self,
        service: &str,
        metric: &str,
        resource: &str,
    ) -> DomainResult<()> {
        let charge = self.profile.capacity(service, metric, resource, 0.0)?;
        if let Some(usage) = self
            .store
            .usage()
            .await?
            .into_iter()
            .find(|u| u.key == charge.key)
            && usage.consumed >= charge.limit
            && charge.action == AdmissionAction::Block
        {
            return Err(brains_domain::DomainError::QuotaExceeded {
                rule: charge.rule,
                reset_at: None,
            });
        }
        Ok(())
    }

    pub async fn settle(&self, reservation: &Reservation, outcome: Settlement) -> DomainResult<()> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let outcome = if matches!(outcome, Settlement::Consume)
            && reservation
                .charges
                .iter()
                .any(|charge| charge.window.end.is_some_and(|end| end <= now))
        {
            Settlement::Uncertain
        } else {
            outcome
        };
        self.store.settle(&reservation.attempt_id, outcome).await
    }

    pub async fn usage(&self) -> DomainResult<Vec<BudgetUsage>> {
        let mut usage = self.store.usage().await?;
        let now = time::OffsetDateTime::now_utc();
        for rule in &self.profile.rules {
            if let Ok(charge) = self.profile.charge(
                &rule.resource.service,
                &rule.measure.metric,
                0.0,
                &self.account,
                UsageEvidence::Bounded,
                now,
            ) && let Some(row) = usage.iter_mut().find(|u| u.key == charge.key)
            {
                row.rule = charge.rule;
                row.limit = charge.limit;
            }
        }
        Ok(usage)
    }

    pub async fn admission(
        &self,
        service: &str,
        metric: &str,
        evidence: UsageEvidence,
    ) -> brains_domain::capabilities::AdmissionState {
        use brains_domain::capabilities::AdmissionState;
        let unavailable = |reason: String| AdmissionState {
            state: "unconfigured".into(),
            reason: Some(reason),
            rule: None,
            limit: None,
            consumed: None,
            reserved: None,
            remaining: None,
            reset_at: None,
        };
        let charges = match self.charges(service, metric, 1.0, evidence) {
            Ok(charges) => charges,
            Err(error) => return unavailable(error.to_string()),
        };
        let usage = match self.usage().await {
            Ok(usage) => usage,
            Err(error) => return unavailable(error.code().into()),
        };
        let exhausted = |charge: &&BudgetCharge| {
            let consumed = usage
                .iter()
                .find(|u| u.key == charge.key)
                .map_or(0.0, |u| u.consumed);
            let reserved = usage
                .iter()
                .filter(|u| u.meter == charge.meter)
                .map(|u| u.reserved)
                .sum::<f64>();
            charge.action == AdmissionAction::Block
                && consumed + reserved + charge.amount > charge.limit
        };
        let failed = charges.iter().find(exhausted);
        let charge = failed.unwrap_or_else(|| {
            charges
                .iter()
                .find(|c| c.measure.metric == metric)
                .expect("primary charge")
        });
        let used = usage.iter().find(|u| u.key == charge.key);
        let consumed = used.map_or(0.0, |u| u.consumed);
        let reserved = usage
            .iter()
            .filter(|u| u.meter == charge.meter)
            .map(|u| u.reserved)
            .sum::<f64>();
        AdmissionState {
            state: if failed.is_some() { "blocked" } else { "ready" }.into(),
            reason: failed.map(|_| "quota_exhausted".into()),
            rule: Some(charge.rule.clone()),
            limit: Some(charge.limit),
            consumed: Some(consumed),
            reserved: Some(reserved),
            remaining: Some((charge.limit - consumed - reserved).max(0.0)),
            reset_at: charge.window.end,
        }
    }
}
