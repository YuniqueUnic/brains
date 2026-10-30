use super::{D1Index, index_err, index_msg};
use brains_domain::{DomainError, DomainResult, resources::UsageEvidence};
use wasm_bindgen::JsValue;

impl D1Index {
    async fn before_query(&self, write: bool) -> DomainResult<()> {
        if let Some(budget) = &self.budget {
            budget
                .debit("d1", if write { "rows_written" } else { "rows_read" }, 0.0)
                .await?;
            if write {
                budget
                    .check_capacity("d1", "storage", "BRAINS_INDEX")
                    .await?;
            }
        }
        Ok(())
    }

    async fn observe_result(
        &self,
        result: &worker::D1Result,
        observed_at: i64,
    ) -> DomainResult<()> {
        if let Some(budget) = &self.budget {
            let meta = result.meta().ok().flatten();
            let counts = meta
                .as_ref()
                .and_then(|meta| meta.rows_read.zip(meta.rows_written));
            let Some((meta, rows)) = meta.zip(counts) else {
                budget.record_unmeasured(observed_at).await?;
                return Err(DomainError::IndexUnavailable(
                    "D1 row counts are unavailable".into(),
                ));
            };
            let now = time::OffsetDateTime::from_unix_timestamp(observed_at)
                .map_err(|error| DomainError::Internal(error.to_string()))?;
            let mut charges = Vec::new();
            for (metric, amount) in [("rows_read", rows.0), ("rows_written", rows.1)] {
                if amount > 0 {
                    charges.extend(budget.measured_charge(budget.profile.charge(
                        "d1",
                        metric,
                        amount as f64,
                        &budget.account,
                        UsageEvidence::Exact,
                        now,
                    ))?);
                }
            }
            if let Some(bytes) = meta.size_after {
                charges.extend(budget.measured_charge(budget.profile.capacity(
                    "d1",
                    "storage",
                    "BRAINS_INDEX",
                    bytes as f64,
                ))?);
            }
            budget.record(charges).await?;
        }
        Ok(())
    }

    async fn received(
        &self,
        result: Result<worker::D1Result, worker::Error>,
        observed_at: i64,
        what: &str,
    ) -> DomainResult<worker::D1Result> {
        match result {
            Ok(result) => {
                self.observe_result(&result, observed_at).await?;
                Ok(result)
            }
            Err(error) => {
                if let Some(budget) = &self.budget {
                    budget.record_unmeasured(observed_at).await?;
                }
                Err(index_err(what, error))
            }
        }
    }

    pub async fn execute(&self, what: &str, sql: &str, params: &[JsValue]) -> DomainResult<()> {
        self.before_query(true).await?;
        let observed_at = time::OffsetDateTime::now_utc().unix_timestamp();
        let result = self
            .db
            .prepare(sql)
            .bind(params)
            .map_err(|e| index_err(what, e))?
            .run()
            .await;
        let result = self.received(result, observed_at, what).await?;
        if !result.success() {
            return Err(index_msg(
                what,
                result.error().unwrap_or_else(|| "D1 query failed".into()),
            ));
        }
        Ok(())
    }

    pub async fn execute_returning<T: for<'de> serde::Deserialize<'de>>(
        &self,
        what: &str,
        sql: &str,
        params: &[JsValue],
    ) -> DomainResult<Vec<T>> {
        self.before_query(true).await?;
        let observed_at = time::OffsetDateTime::now_utc().unix_timestamp();
        let result = self
            .db
            .prepare(sql)
            .bind(params)
            .map_err(|e| index_err(what, e))?
            .all()
            .await;
        let result = self.received(result, observed_at, what).await?;
        if !result.success() {
            return Err(index_msg(
                what,
                result.error().unwrap_or_else(|| "D1 query failed".into()),
            ));
        }
        result.results::<T>().map_err(|e| index_err(what, e))
    }

    pub async fn query<T: for<'de> serde::Deserialize<'de>>(
        &self,
        what: &str,
        sql: &str,
        params: &[JsValue],
    ) -> DomainResult<Vec<T>> {
        self.before_query(false).await?;
        let observed_at = time::OffsetDateTime::now_utc().unix_timestamp();
        let result = self
            .db
            .prepare(sql)
            .bind(params)
            .map_err(|e| index_err(what, e))?
            .all()
            .await;
        let result = self.received(result, observed_at, what).await?;
        if !result.success() {
            return Err(index_msg(
                what,
                result.error().unwrap_or_else(|| "D1 query failed".into()),
            ));
        }
        result.results::<T>().map_err(|e| index_err(what, e))
    }
}
