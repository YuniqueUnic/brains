//! Atomic usage accounting; implementations own transactions, not policy.

use brains_domain::{
    DomainResult,
    resources::{BudgetCharge, BudgetUsage, Reservation, ResourceSettings, Settlement},
};
use futures::future::BoxFuture;

/// Cumulative bookkeeping rows observed at one operation-start timestamp.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccountingUsage {
    pub observed_at: i64,
    pub rows_read: u64,
    pub rows_written: u64,
    pub unmeasured_operations: u64,
}

pub trait ResourceSettingsStore: Send + Sync {
    fn load(&self) -> BoxFuture<'_, DomainResult<Option<ResourceSettings>>>;
    fn save<'a>(
        &'a self,
        previous: Option<&'a str>,
        next: &'a ResourceSettings,
    ) -> BoxFuture<'a, DomainResult<()>>;
}

pub trait UsageStore: Send + Sync {
    /// Last accounted billing boundary, including consumed historical periods.
    fn latest_period_end<'a>(
        &'a self,
        rules: &'a [String],
    ) -> BoxFuture<'a, DomainResult<Option<i64>>> {
        Box::pin(async move {
            Ok(self
                .usage()
                .await?
                .into_iter()
                .filter(|u| rules.contains(&u.rule) && (u.consumed > 0.0 || u.reserved > 0.0))
                .filter_map(|u| u.reset_at)
                .max())
        })
    }
    /// Bound for the final accounting write, not an allowance per operation.
    /// Native stores do not incur D1 bookkeeping costs.
    fn accounting_bounds(&self) -> (u64, u64) {
        (0, 0)
    }
    /// Closing cost for these resolved keys. Stores may use a smaller bound
    /// when successful statements have already established the rows exist.
    fn accounting_bounds_for(&self, _charges: &[BudgetCharge]) -> (u64, u64) {
        self.accounting_bounds()
    }
    /// Cumulative measured bookkeeping rows for this store's request lifetime.
    fn accounting_usage(&self) -> Vec<AccountingUsage> {
        vec![]
    }
    /// Check all projected charges without consuming them. D1 implements this
    /// as one read-only statement; other stores retain their existing behavior.
    fn check<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        self.debit(charges)
    }
    /// Persist the remaining accounting charge and return this write's own
    /// measured cost. It must fit accounting_bounds_for to close without recursion.
    fn flush_accounting<'a>(
        &'a self,
        charges: &'a [BudgetCharge],
        observed_at: i64,
    ) -> BoxFuture<'a, DomainResult<AccountingUsage>> {
        Box::pin(async move {
            self.observe(charges).await?;
            Ok(AccountingUsage {
                observed_at,
                ..Default::default()
            })
        })
    }
    fn debit<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>>;
    fn observe<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>>;
    fn reserve<'a>(&'a self, reservation: &'a Reservation) -> BoxFuture<'a, DomainResult<()>>;
    fn settle<'a>(
        &'a self,
        attempt_id: &'a str,
        outcome: Settlement,
    ) -> BoxFuture<'a, DomainResult<()>>;
    fn usage(&self) -> BoxFuture<'_, DomainResult<Vec<BudgetUsage>>>;
}
