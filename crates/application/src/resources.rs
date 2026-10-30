//! Budget orchestration: provider facts resolve a charge; a store admits it.

use std::sync::Arc;

use brains_domain::{DomainResult, HexId, resources::*};
use brains_ports::resources::UsageStore;

pub struct ResourceBudget {
    pub profile: ResourceProfile,
    pub account: String,
    store: Arc<dyn UsageStore>,
    enforced: bool,
    accounted: futures::lock::Mutex<(u64, u64)>,
}

impl ResourceBudget {
    pub fn with_enforcement(mut self, enforced: bool) -> Self {
        self.enforced = enforced;
        self
    }
    pub async fn observe_request(&self) -> DomainResult<()> {
        let charges = self.charges("workers", "requests", 1.0, UsageEvidence::Bounded)?;
        self.apply_accounted(charges, false).await
    }
    pub fn new(profile: ResourceProfile, account: String, store: Arc<dyn UsageStore>) -> Self {
        Self {
            profile,
            account,
            store,
            enforced: true,
            accounted: futures::lock::Mutex::new((0, 0)),
        }
    }

    pub async fn reserve(
        &self,
        service: &str,
        metric: &str,
        amount: f64,
        evidence: UsageEvidence,
    ) -> DomainResult<Reservation> {
        let primary = self.charges(service, metric, amount, evidence)?;
        self.apply_accounted(vec![], false).await?;
        self.check_accounted(vec![]).await?;
        // Previously incurred bookkeeping is never a refundable reservation.
        // Zero-amount D1 guards still participate in the atomic admission.
        let charges = self.with_accounting(primary, (0, 0), UsageEvidence::Exact)?;
        let reservation = Reservation {
            attempt_id: HexId::generate().as_str().to_owned(),
            charges,
        };
        if !reservation.charges.is_empty() {
            self.store.reserve(&reservation).await?;
        }
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
        let mut observed;
        let profile = if !self.enforced
            && matches!(evidence, UsageEvidence::Unknown | UsageEvidence::Estimated)
        {
            observed = self.profile.clone();
            observed.policy.when_charge_unknown = Some(AdmissionAction::Observe);
            for rule in &mut observed.rules {
                rule.policy.when_charge_unknown = Some(AdmissionAction::Observe);
            }
            &observed
        } else {
            &self.profile
        };
        let Some(charge) = self.measured_charge(profile.charge(
            service,
            metric,
            amount,
            &self.account,
            evidence,
            now,
        ))?
        else {
            return Ok(vec![]);
        };
        Ok(vec![charge])
    }

    /// Missing meter facts remain unmeasured only when enforcement is off.
    pub fn measured_charge(
        &self,
        result: DomainResult<BudgetCharge>,
    ) -> DomainResult<Option<BudgetCharge>> {
        match result {
            Ok(mut charge) => {
                if !self.enforced {
                    charge.action = AdmissionAction::Observe;
                }
                Ok(Some(charge))
            }
            Err(brains_domain::DomainError::ResourcePolicy(_)) if !self.enforced => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Debit a known attempt before dispatch; a cancelled dispatch overcounts safely.
    pub async fn debit(&self, service: &str, metric: &str, amount: f64) -> DomainResult<()> {
        let charges = self.charges(service, metric, amount, UsageEvidence::Bounded)?;
        if amount == 0.0 {
            return self.check_accounted(charges).await;
        }
        self.apply_accounted(charges, self.enforced).await
    }

    /// Record completed I/O and previously measured bookkeeping together.
    pub async fn record(&self, charges: Vec<BudgetCharge>) -> DomainResult<()> {
        self.apply_accounted(charges, false).await
    }

    async fn check_accounted(&self, charges: Vec<BudgetCharge>) -> DomainResult<()> {
        let accounted = self.accounted.lock().await;
        let usage = self.store.accounting_usage();
        let charges = self.with_accounting(
            charges,
            (usage.0 - accounted.0, usage.1 - accounted.1),
            UsageEvidence::Exact,
        )?;
        if charges.is_empty() {
            return Ok(());
        }
        self.store.check(&charges).await
    }

    async fn apply_accounted(&self, charges: Vec<BudgetCharge>, enforce: bool) -> DomainResult<()> {
        // Serialize acknowledgement, not provider I/O. A failed admission must
        // leave the receipt pending for the end-of-request accounting write.
        let mut accounted = self.accounted.lock().await;
        let usage = self.store.accounting_usage();
        let charges = self.with_accounting(
            charges,
            (usage.0 - accounted.0, usage.1 - accounted.1),
            UsageEvidence::Exact,
        )?;
        if charges
            .iter()
            .any(|charge| charge.amount > 0.0 || charge.mode == ChargeMode::Gauge)
        {
            if enforce {
                self.store.debit(&charges).await?;
            } else {
                self.store.observe(&charges).await?;
            }
        }
        *accounted = usage;
        Ok(())
    }

    /// Close a completed request or bounded background scope without recursively
    /// charging the final accounting write. Repeating an idle flush is a no-op.
    pub async fn flush_accounting(&self) -> DomainResult<()> {
        let mut accounted = self.accounted.lock().await;
        let usage = self.store.accounting_usage();
        if usage == *accounted {
            return Ok(());
        }
        // Freeze the keys before choosing the closing cost: an old window's
        // materialized rows do not establish that the new window exists.
        let mut charges = self.with_accounting(
            vec![],
            (usage.0 - accounted.0, usage.1 - accounted.1),
            UsageEvidence::Bounded,
        )?;
        let bound = self.store.accounting_bounds_for(&charges);
        for charge in &mut charges {
            charge.amount += match charge.measure.metric.as_str() {
                "rows_read" => bound.0,
                "rows_written" => bound.1,
                _ => 0,
            } as f64;
        }
        let closing_cost = self.store.flush_accounting(&charges).await?;
        // A background operation may have completed during this write. Only
        // acknowledge the closing write's own receipt, not a newer snapshot.
        *accounted = (usage.0 + closing_cost.0, usage.1 + closing_cost.1);
        Ok(())
    }

    fn with_accounting(
        &self,
        mut charges: Vec<BudgetCharge>,
        usage: (u64, u64),
        evidence: UsageEvidence,
    ) -> DomainResult<Vec<BudgetCharge>> {
        let now = time::OffsetDateTime::now_utc();
        let (reads, writes) = usage;
        for (metric, amount) in [("rows_read", reads), ("rows_written", writes)] {
            if self.store.accounting_bounds() != (0, 0) || amount > 0 {
                charges.extend(self.measured_charge(self.profile.charge(
                    "d1",
                    metric,
                    amount as f64,
                    &self.account,
                    evidence,
                    now,
                ))?);
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
        if !self.enforced {
            return Ok(());
        }
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
        if reservation.charges.is_empty() {
            return Ok(());
        }
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
        if !self.enforced {
            let mut state = unavailable(
                "Application budgets are not enforced; measurable usage is observed".into(),
            );
            state.state = "observe".into();
            return state;
        }
        let charges = match self
            .charges(service, metric, 1.0, evidence)
            .and_then(|charges| self.with_accounting(charges, (0, 0), UsageEvidence::Exact))
        {
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
