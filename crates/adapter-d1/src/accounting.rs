//! Request-local bookkeeping facts, independent of profile/window policy.

use brains_domain::{DomainError, DomainResult};
use brains_ports::resources::AccountingUsage;
use std::{collections::BTreeMap, sync::Mutex};
use worker::D1Result;

#[derive(Default)]
pub struct D1Accounting {
    rows: Mutex<BTreeMap<i64, AccountingUsage>>,
}

impl D1Accounting {
    pub fn usage(&self) -> Vec<AccountingUsage> {
        self.rows
            .lock()
            .expect("D1 accounting lock")
            .values()
            .copied()
            .collect()
    }

    pub(crate) fn unmeasured(&self, observed_at: i64) {
        let mut rows = self.rows.lock().expect("D1 accounting lock");
        rows.entry(observed_at)
            .or_insert(AccountingUsage {
                observed_at,
                ..Default::default()
            })
            .unmeasured_operations += 1;
    }

    pub fn record(&self, result: &D1Result, observed_at: i64) -> DomainResult<AccountingUsage> {
        let counts = result
            .meta()
            .ok()
            .flatten()
            .and_then(|meta| meta.rows_read.zip(meta.rows_written));
        let Some((reads, writes)) = counts else {
            self.unmeasured(observed_at);
            return Err(DomainError::IndexUnavailable(
                "D1 bookkeeping row counts missing".into(),
            ));
        };
        let receipt = AccountingUsage {
            observed_at,
            rows_read: reads as u64,
            rows_written: writes as u64,
            ..Default::default()
        };
        let mut total = self.rows.lock().expect("D1 accounting lock");
        let bucket = total.entry(observed_at).or_insert(AccountingUsage {
            observed_at,
            ..Default::default()
        });
        bucket.rows_read += receipt.rows_read;
        bucket.rows_written += receipt.rows_written;
        Ok(receipt)
    }
}
