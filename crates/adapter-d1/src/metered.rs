use super::{D1Index, index_err, index_msg};
use brains_domain::{DomainResult, resources::UsageEvidence};
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

    async fn observe_result(&self, result: &worker::D1Result) -> DomainResult<()> {
        if let Some(budget) = &self.budget
            && let Some(meta) = result.meta().map_err(|e| index_err("D1 metadata", e))?
        {
            let now = time::OffsetDateTime::now_utc();
            let mut charges = Vec::new();
            for (metric, amount) in [
                ("rows_read", meta.rows_read.unwrap_or(0)),
                ("rows_written", meta.rows_written.unwrap_or(0)),
            ] {
                if amount > 0 {
                    charges.push(budget.profile.charge(
                        "d1",
                        metric,
                        amount as f64,
                        &budget.account,
                        UsageEvidence::Exact,
                        now,
                    )?);
                }
            }
            if let Some(bytes) = meta.size_after {
                charges.push(budget.profile.capacity(
                    "d1",
                    "storage",
                    "BRAINS_INDEX",
                    bytes as f64,
                )?);
            }
            budget.record(charges).await?;
        }
        Ok(())
    }

    pub async fn execute(&self, what: &str, sql: &str, params: &[JsValue]) -> DomainResult<()> {
        self.before_query(true).await?;
        let result = self
            .db
            .prepare(sql)
            .bind(params)
            .map_err(|e| index_err(what, e))?
            .run()
            .await
            .map_err(|e| index_err(what, e))?;
        self.observe_result(&result).await?;
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
        let result = self
            .db
            .prepare(sql)
            .bind(params)
            .map_err(|e| index_err(what, e))?
            .all()
            .await
            .map_err(|e| index_err(what, e))?;
        self.observe_result(&result).await?;
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
        let result = self
            .db
            .prepare(sql)
            .bind(params)
            .map_err(|e| index_err(what, e))?
            .all()
            .await
            .map_err(|e| index_err(what, e))?;
        self.observe_result(&result).await?;
        if !result.success() {
            return Err(index_msg(
                what,
                result.error().unwrap_or_else(|| "D1 query failed".into()),
            ));
        }
        result.results::<T>().map_err(|e| index_err(what, e))
    }
}
