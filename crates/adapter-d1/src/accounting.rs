//! Request-local receipts for bookkeeping SQL, separate from budget policy.

use std::sync::Mutex;

use brains_domain::{DomainError, DomainResult};
use worker::D1Result;

#[derive(Default)]
pub struct D1Accounting {
    rows: Mutex<(u64, u64)>,
}

impl D1Accounting {
    pub fn usage(&self) -> (u64, u64) {
        *self.rows.lock().expect("D1 accounting lock")
    }

    pub fn record(&self, result: &D1Result) -> DomainResult<(u64, u64)> {
        let meta = result
            .meta()
            .map_err(|error| super::index_err("bookkeeping metadata", error))?
            .ok_or_else(|| {
                DomainError::IndexUnavailable("D1 bookkeeping metadata missing".into())
            })?;
        let rows = meta.rows_read.zip(meta.rows_written).ok_or_else(|| {
            DomainError::IndexUnavailable("D1 bookkeeping row counts missing".into())
        })?;
        let receipt = (rows.0 as u64, rows.1 as u64);
        let mut total = self.rows.lock().expect("D1 accounting lock");
        total.0 += receipt.0;
        total.1 += receipt.1;
        Ok(receipt)
    }
}
