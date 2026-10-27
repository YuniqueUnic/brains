//! Atomic usage accounting; implementations own transactions, not policy.

use brains_domain::{
    DomainResult,
    resources::{BudgetCharge, BudgetUsage, Reservation, Settlement},
};
use futures::future::BoxFuture;

pub trait UsageStore: Send + Sync {
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
