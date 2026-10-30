//! Budget orchestration: provider facts resolve a charge; a store admits it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::{collections::BTreeMap, sync::Arc};

use brains_domain::{DomainResult, HexId, resources::*};
use brains_ports::resources::{AccountingUsage, UsageStore};

pub struct ResourceBudget {
    pub profile: ResourceProfile,
    pub account: String,
    store: Arc<dyn UsageStore>,
    enforced: bool,
    accounted: futures::lock::Mutex<BTreeMap<i64, AccountingUsage>>,
    closing_failed: AtomicBool,
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
            accounted: futures::lock::Mutex::new(BTreeMap::new()),
            closing_failed: AtomicBool::new(false),
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
        let charges = self.with_accounting(primary, &[], UsageEvidence::Exact, now())?;
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
                    charge.block_unknown = false;
                }
                Ok(Some(charge))
            }
            Err(brains_domain::DomainError::ResourcePolicy(_)) if !self.enforced => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Debit a known attempt before dispatch; a cancelled dispatch overcounts safely.
    pub async fn debit(&self, service: &str, metric: &str, amount: f64) -> DomainResult<()> {
        if self.enforced {
            self.settle_previous_accounting().await?;
        }
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

    /// Record incomplete measurement without inventing a row amount.
    pub async fn record_unmeasured(&self, observed_at: i64) -> DomainResult<()> {
        let receipt = AccountingUsage {
            observed_at,
            unmeasured_operations: 1,
            ..Default::default()
        };
        let charges =
            self.with_accounting(vec![], &[receipt], UsageEvidence::Exact, observed_at)?;
        self.record(charges).await
    }

    async fn check_accounted(&self, charges: Vec<BudgetCharge>) -> DomainResult<()> {
        let accounted = self.accounted.lock().await;
        let usage = self.store.accounting_usage();
        let charges = self.with_accounting(
            charges,
            &pending(&usage, &accounted),
            UsageEvidence::Exact,
            now(),
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
            &pending(&usage, &accounted),
            UsageEvidence::Exact,
            now(),
        )?;
        if charges.iter().any(|charge| {
            charge.amount > 0.0
                || charge.mode == ChargeMode::Gauge
                || charge.evidence == UsageEvidence::Unknown
        }) {
            if enforce {
                if let Err(error) = self.store.debit(&charges).await {
                    if !matches!(
                        error,
                        brains_domain::DomainError::QuotaExceeded { .. }
                            | brains_domain::DomainError::ResourcePolicy(_)
                    ) {
                        *accounted = usage
                            .into_iter()
                            .map(|row| (row.observed_at, row))
                            .collect();
                    }
                    return Err(error);
                }
            } else {
                if let Err(error) = self.store.observe(&charges).await {
                    *accounted = usage
                        .into_iter()
                        .map(|row| (row.observed_at, row))
                        .collect();
                    return Err(error);
                }
            }
        }
        *accounted = usage
            .into_iter()
            .map(|row| (row.observed_at, row))
            .collect();
        Ok(())
    }

    /// Close one completed scope. Its own SQL cost belongs to the closing
    /// dispatch window; pending receipts retain their original dispatch time.
    pub async fn flush_accounting(&self) -> DomainResult<()> {
        self.close_accounting(now()).await
    }

    pub(crate) async fn close_accounting(&self, observed_at: i64) -> DomainResult<()> {
        let mut accounted = self.accounted.lock().await;
        if self.closing_failed.load(Ordering::Relaxed) {
            return Err(brains_domain::DomainError::IndexUnavailable(
                "final accounting has an unresolved commit; it cannot be replayed".into(),
            ));
        }
        let usage = self.store.accounting_usage();
        let pending = pending(&usage, &accounted);
        if pending.is_empty() {
            return Ok(());
        }
        let guards = self.with_accounting(vec![], &[], UsageEvidence::Bounded, observed_at)?;
        let mut charges = self.with_accounting(
            guards.clone(),
            &pending,
            UsageEvidence::Bounded,
            observed_at,
        )?;
        let bound = self.store.accounting_bounds_for(&charges);
        for charge in &mut charges {
            if guards.iter().any(|guard| guard.key == charge.key) {
                charge.amount += match charge.measure.metric.as_str() {
                    "rows_read" => bound.0,
                    "rows_written" => bound.1,
                    _ => 0,
                } as f64;
            }
        }
        let result = self.store.flush_accounting(&charges, observed_at).await;
        // A missing response can follow a committed write. Do not replay its
        // input receipts; newly returned facts remain independently pending.
        *accounted = usage
            .into_iter()
            .map(|row| (row.observed_at, row))
            .collect();
        let closing_cost = result.inspect_err(|_| {
            self.closing_failed.store(true, Ordering::Relaxed);
        })?;
        let receipt = accounted
            .entry(closing_cost.observed_at)
            .or_insert(AccountingUsage {
                observed_at: closing_cost.observed_at,
                ..Default::default()
            });
        receipt.rows_read += closing_cost.rows_read;
        receipt.rows_written += closing_cost.rows_written;
        receipt.unmeasured_operations += closing_cost.unmeasured_operations;
        Ok(())
    }

    async fn settle_previous_accounting(&self) -> DomainResult<()> {
        let observed_at = now();
        let mut accounted = self.accounted.lock().await;
        let usage = self.store.accounting_usage();
        let pending = pending(&usage, &accounted);
        if pending.is_empty() {
            return Ok(());
        }
        let charges = self.with_accounting(vec![], &pending, UsageEvidence::Exact, observed_at)?;
        if charges.iter().any(|charge| {
            (charge.amount > 0.0 || charge.evidence == UsageEvidence::Unknown)
                && charge.window.end.is_some_and(|end| end <= observed_at)
        }) {
            // Historical, already-incurred work cannot gate a new window.
            let result = self.store.observe(&charges).await;
            *accounted = usage
                .into_iter()
                .map(|row| (row.observed_at, row))
                .collect();
            result?;
        }
        Ok(())
    }

    fn with_accounting(
        &self,
        mut charges: Vec<BudgetCharge>,
        usage: &[AccountingUsage],
        evidence: UsageEvidence,
        observed_at: i64,
    ) -> DomainResult<Vec<BudgetCharge>> {
        let guard = AccountingUsage {
            observed_at,
            ..Default::default()
        };
        for receipt in usage.iter().chain(std::iter::once(&guard)) {
            let at = time::OffsetDateTime::from_unix_timestamp(receipt.observed_at)
                .map_err(|error| brains_domain::DomainError::Internal(error.to_string()))?;
            for (metric, amount) in [
                ("rows_read", receipt.rows_read),
                ("rows_written", receipt.rows_written),
            ] {
                if (self.store.accounting_bounds() != (0, 0)
                    || amount > 0
                    || receipt.unmeasured_operations > 0)
                    && let Some(mut charge) = self.measured_charge(self.profile.charge(
                        "d1",
                        metric,
                        amount as f64,
                        &self.account,
                        evidence,
                        at,
                    ))?
                {
                    if receipt.unmeasured_operations > 0 {
                        charge.evidence = UsageEvidence::Unknown;
                    }
                    charges.push(charge);
                }
            }
        }
        let mut merged = std::collections::BTreeMap::<String, BudgetCharge>::new();
        for charge in charges {
            if let Some(existing) = merged.get_mut(&charge.key) {
                existing.amount += charge.amount;
                existing.evidence = existing.evidence.max(charge.evidence);
                existing.block_unknown |= charge.block_unknown;
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
            && reservation.charges.iter().any(|charge| {
                charge.amount > 0.0 && charge.window.end.is_some_and(|end| end <= now)
            }) {
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
            .and_then(|charges| self.with_accounting(charges, &[], UsageEvidence::Exact, now()))
        {
            Ok(charges) => charges,
            Err(error) => return unavailable(error.to_string()),
        };
        let usage = match self.usage().await {
            Ok(usage) => usage,
            Err(error) => return unavailable(error.code().into()),
        };
        let unknown = |charge: &&BudgetCharge| {
            charge.block_unknown
                && (charge.evidence == UsageEvidence::Unknown
                    || usage
                        .iter()
                        .any(|row| row.key == charge.key && row.evidence == UsageEvidence::Unknown))
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
        let unmeasured = charges.iter().find(unknown);
        let failed = unmeasured.or_else(|| charges.iter().find(exhausted));
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
            reason: failed.map(|_| {
                if unmeasured.is_some() {
                    "usage_unknown"
                } else {
                    "quota_exhausted"
                }
                .into()
            }),
            rule: Some(charge.rule.clone()),
            limit: Some(charge.limit),
            consumed: Some(consumed),
            reserved: Some(reserved),
            remaining: Some((charge.limit - consumed - reserved).max(0.0)),
            reset_at: charge.window.end,
        }
    }
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn pending(
    usage: &[AccountingUsage],
    accounted: &BTreeMap<i64, AccountingUsage>,
) -> Vec<AccountingUsage> {
    usage
        .iter()
        .filter_map(|row| {
            let previous = accounted.get(&row.observed_at).copied().unwrap_or_default();
            let delta = AccountingUsage {
                observed_at: row.observed_at,
                rows_read: row.rows_read - previous.rows_read,
                rows_written: row.rows_written - previous.rows_written,
                unmeasured_operations: row.unmeasured_operations - previous.unmeasured_operations,
            };
            (delta.rows_read > 0 || delta.rows_written > 0 || delta.unmeasured_operations > 0)
                .then_some(delta)
        })
        .collect()
}
