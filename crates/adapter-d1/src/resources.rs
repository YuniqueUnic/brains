use brains_domain::{DomainError, DomainResult, resources::*};
use brains_ports::resources::UsageStore;
use futures::future::BoxFuture;
use wasm_bindgen::JsValue;
use worker::{D1Database, send::SendFuture};

use super::{D1Accounting, index_err, query_rows};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

const EXISTING_ROWS_BOUND: (u64, u64) = (2, 2);

pub struct D1UsageStore {
    db: D1Database,
    accounting: Arc<D1Accounting>,
    persisted_keys: Mutex<BTreeSet<String>>,
}

impl D1UsageStore {
    pub fn new(db: D1Database) -> Self {
        Self {
            db,
            accounting: Arc::default(),
            persisted_keys: Mutex::default(),
        }
    }

    pub fn with_accounting(mut self, accounting: Arc<D1Accounting>) -> Self {
        self.accounting = accounting;
        self
    }
}

impl UsageStore for D1UsageStore {
    fn latest_period_end<'a>(
        &'a self,
        rules: &'a [String],
    ) -> BoxFuture<'a, DomainResult<Option<i64>>> {
        Box::pin(SendFuture::new(async move {
            #[derive(serde::Deserialize)]
            struct Row {
                end: Option<i64>,
            }
            let json =
                serde_json::to_string(rules).map_err(|e| DomainError::Internal(e.to_string()))?;
            let rows: Vec<Row> = query_rows(
                &self.db,
                &self.accounting,
                "accounted period boundary",
                "SELECT MAX(reset_at) AS \"end\" FROM resource_usage WHERE rule IN (SELECT value FROM json_each(?1)) AND (consumed>0 OR reserved>0)",
                &[JsValue::from_str(&json)],
            ).await?;
            Ok(rows.first().and_then(|r| r.end))
        }))
    }
    fn accounting_bounds(&self) -> (u64, u64) {
        // The closing batch has two primary-key upserts, no scans or triggers.
        // Each can read at most two rows and insert one table row plus the
        // primary-key, meter, reset and rule indexes. Existing rows cost less.
        (4, 10)
    }

    fn accounting_bounds_for(&self, charges: &[BudgetCharge]) -> (u64, u64) {
        if self.closing_rows_exist(charges) {
            // Two primary-key updates of unindexed values; no index writes.
            EXISTING_ROWS_BOUND
        } else {
            self.accounting_bounds()
        }
    }

    fn accounting_usage(&self) -> (u64, u64) {
        self.accounting.usage()
    }

    fn check<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(async move {
            if charges.is_empty() {
                return Ok(());
            }
            #[derive(serde::Deserialize)]
            struct Row {
                allowed: i64,
            }
            let json = serde_json::to_string(charges)
                .map_err(|error| DomainError::Internal(error.to_string()))?;
            let rows: Vec<Row> = query_rows(
                &self.db,
                &self.accounting,
                "usage admission check",
                include_str!("sql/check.sql"),
                &[JsValue::from_str(&json)],
            )
            .await?;
            if rows.first().is_some_and(|row| row.allowed != 0) {
                Ok(())
            } else {
                self.exhausted(charges).await
            }
        }))
    }

    fn flush_accounting<'a>(
        &'a self,
        charges: &'a [BudgetCharge],
    ) -> BoxFuture<'a, DomainResult<(u64, u64)>> {
        Box::pin(SendFuture::new(async move {
            let sql = if self.closing_rows_exist(charges) {
                include_str!("sql/accounting-update.sql")
            } else {
                include_str!("sql/accounting.sql")
            };
            let mut statements = Vec::new();
            for charge in charges {
                let json = serde_json::to_string(charge)
                    .map_err(|error| DomainError::Internal(error.to_string()))?;
                statements.push(
                    self.db
                        .prepare(sql)
                        .bind(&[JsValue::from_str(&json)])
                        .map_err(|error| index_err("final accounting", error))?,
                );
            }
            if statements.is_empty() {
                return Ok((0, 0));
            }
            let results = self
                .db
                .batch(statements)
                .await
                .map_err(|error| index_err("final accounting", error))?;
            let mut cost = (0, 0);
            for result in results {
                let receipt = self.accounting.record(&result)?;
                cost.0 += receipt.0;
                cost.1 += receipt.1;
                if !result.success() {
                    return Err(DomainError::IndexUnavailable(
                        "final accounting failed".into(),
                    ));
                }
            }
            self.persisted_keys
                .lock()
                .expect("D1 persisted keys")
                .extend(charges.iter().map(|charge| charge.key.clone()));
            Ok(cost)
        }))
    }

    fn debit<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(
            async move { self.apply(charges, true).await },
        ))
    }

    fn observe<'a>(&'a self, charges: &'a [BudgetCharge]) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(
            async move { self.apply(charges, false).await },
        ))
    }

    fn reserve<'a>(&'a self, reservation: &'a Reservation) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(async move {
            let payload = serde_json::to_string(&reservation.charges)
                .map_err(|e| DomainError::Internal(e.to_string()))?;
            let id = JsValue::from_str(&reservation.attempt_id);
            let json = JsValue::from_str(&payload);
            let prepare = |sql: &str, params: &[JsValue]| {
                self.db
                    .prepare(sql)
                    .bind(params)
                    .map_err(|e| index_err("reserve usage", e))
            };
            let statements = vec![
                prepare(
                    "INSERT INTO resource_usage(key,meter,rule,unit,evidence,limit_value,reset_at) SELECT json_extract(value,'$.key'),json_extract(value,'$.meter'),json_extract(value,'$.rule'),json_extract(value,'$.measure.unit'),json_extract(value,'$.evidence'),json_extract(value,'$.limit'),json_extract(value,'$.window.end') FROM json_each(?1) WHERE 1 ON CONFLICT(key) DO UPDATE SET rule=excluded.rule, unit=excluded.unit, evidence=CASE WHEN resource_usage.evidence='unknown' OR excluded.evidence='unknown' THEN 'unknown' WHEN resource_usage.evidence='estimated' OR excluded.evidence='estimated' THEN 'estimated' WHEN resource_usage.evidence='bounded' OR excluded.evidence='bounded' THEN 'bounded' ELSE 'exact' END, limit_value=excluded.limit_value",
                    std::slice::from_ref(&json),
                )?,
                prepare(
                    "INSERT INTO resource_attempts(id,payload,state) SELECT ?1,?2,'reserved' WHERE NOT EXISTS (SELECT 1 FROM json_each(?2) c JOIN resource_usage u ON u.key=json_extract(c.value,'$.key') WHERE json_extract(c.value,'$.action')='block' AND u.consumed+COALESCE((SELECT SUM(reserved) FROM resource_usage WHERE meter=u.meter AND reserved>0),0)+json_extract(c.value,'$.amount')>json_extract(c.value,'$.limit')) ON CONFLICT(id) DO NOTHING",
                    &[id.clone(), json.clone()],
                )?,
                prepare(
                    "INSERT INTO resource_charges(attempt_id,meter_key,amount) SELECT ?1,json_extract(value,'$.key'),json_extract(value,'$.amount') FROM json_each(?2) WHERE EXISTS (SELECT 1 FROM resource_attempts WHERE id=?1 AND payload=?2 AND state='reserved') ON CONFLICT(attempt_id,meter_key) DO NOTHING",
                    &[id.clone(), json.clone()],
                )?,
            ];
            let results = self
                .db
                .batch(statements)
                .await
                .map_err(|e| index_err("reserve usage", e))?;
            for result in &results {
                self.accounting.record(result)?;
            }
            if results.iter().any(|r| !r.success()) {
                return Err(DomainError::IndexUnavailable(
                    "resource reservation transaction failed".into(),
                ));
            }
            #[derive(serde::Deserialize)]
            struct Attempt {
                payload: String,
            }
            let existing: Vec<Attempt> = query_rows(
                &self.db,
                &self.accounting,
                "read reservation",
                "SELECT payload FROM resource_attempts WHERE id=?1",
                &[id],
            )
            .await?;
            match existing.first() {
                Some(row) if row.payload == payload => Ok(()),
                Some(_) => Err(DomainError::InvalidArgument(
                    "attempt ID reused for different quantities".into(),
                )),
                None => self.exhausted(&reservation.charges).await,
            }
        }))
    }

    fn settle<'a>(
        &'a self,
        attempt_id: &'a str,
        outcome: Settlement,
    ) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(async move {
            let state = match outcome {
                Settlement::Consume => "consumed",
                Settlement::Release => "released",
                Settlement::Uncertain => "uncertain",
            };
            super::run(&self.db,&self.accounting,"settle usage","UPDATE resource_attempts SET state=?2,updated_at=unixepoch() WHERE id=?1 AND state IN ('reserved','uncertain')", &[JsValue::from_str(attempt_id),JsValue::from_str(state)]).await
        }))
    }

    fn usage(&self) -> BoxFuture<'_, DomainResult<Vec<BudgetUsage>>> {
        Box::pin(SendFuture::new(query_rows(
            &self.db,
            &self.accounting,
            "resource usage",
            "SELECT key,meter,rule,unit,evidence,consumed,reserved,limit_value AS \"limit\",reset_at FROM resource_usage WHERE reset_at IS NULL OR reset_at>unixepoch() OR reserved>0 ORDER BY rule",
            &[],
        )))
    }
}

impl D1UsageStore {
    fn closing_rows_exist(&self, charges: &[BudgetCharge]) -> bool {
        let keys = self.persisted_keys.lock().expect("D1 persisted keys");
        charges.len() == 2 && charges.iter().all(|charge| keys.contains(&charge.key))
    }

    async fn exhausted(&self, charges: &[BudgetCharge]) -> DomainResult<()> {
        let usage = self.usage().await?;
        for charge in charges {
            let consumed = usage
                .iter()
                .find(|row| row.key == charge.key)
                .map_or(0.0, |row| row.consumed)
                + usage
                    .iter()
                    .filter(|row| row.meter == charge.meter)
                    .map(|row| row.reserved)
                    .sum::<f64>();
            if charge.action == AdmissionAction::Block && consumed + charge.amount > charge.limit {
                return Err(DomainError::QuotaExceeded {
                    rule: charge.rule.clone(),
                    reset_at: charge.window.end,
                });
            }
        }
        Err(DomainError::Internal(
            "quota admission failed without an exhausted meter".into(),
        ))
    }

    async fn apply(&self, charges: &[BudgetCharge], enforce: bool) -> DomainResult<()> {
        if charges.is_empty() {
            return Ok(());
        }
        let payload =
            serde_json::to_string(charges).map_err(|e| DomainError::Internal(e.to_string()))?;
        // Materialize admission once so every meter is applied, or none is.
        let sql = include_str!("sql/debit.sql");
        let result = self
            .db
            .prepare(sql)
            .bind(&[
                JsValue::from_str(&payload),
                JsValue::from_f64(if enforce { 1.0 } else { 0.0 }),
            ])
            .map_err(|e| index_err("usage debit", e))?
            .all()
            .await
            .map_err(|e| index_err("usage debit", e))?;
        self.accounting.record(&result)?;
        if !result.success() {
            return Err(DomainError::IndexUnavailable(
                result
                    .error()
                    .unwrap_or_else(|| "usage debit failed".into()),
            ));
        }
        #[derive(serde::Deserialize)]
        struct Applied {
            key: String,
        }
        let applied = result
            .results::<Applied>()
            .map_err(|e| index_err("usage debit result", e))?;
        if applied.len() != charges.len() {
            return Err(self.exhausted(charges).await.err().unwrap_or_else(|| {
                DomainError::Internal("quota transaction applied a partial charge set".into())
            }));
        }
        self.persisted_keys
            .lock()
            .expect("D1 persisted keys")
            .extend(applied.into_iter().map(|row| row.key));
        Ok(())
    }
}
