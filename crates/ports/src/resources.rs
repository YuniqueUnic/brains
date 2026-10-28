//! Atomic usage accounting; implementations own transactions, not policy.

use brains_domain::{
    DomainResult,
    resources::{BudgetCharge, BudgetUsage, Reservation, ResourceSettings, Settlement},
};
use futures::future::BoxFuture;

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
    /// Conservative bound for this ledger's reserve/settle SQL itself.
    fn accounting_bounds(&self) -> (u64, u64) {
        (0, 0)
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
