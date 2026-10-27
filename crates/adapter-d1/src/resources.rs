use brains_domain::{DomainError, DomainResult, resources::*};
use brains_ports::resources::UsageStore;
use futures::future::BoxFuture;
use wasm_bindgen::JsValue;
use worker::{D1Database, send::SendFuture};

use super::{index_err, query_rows};

pub struct D1UsageStore {
    db: D1Database,
}

impl D1UsageStore {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

impl UsageStore for D1UsageStore {
    fn accounting_bounds(&self) -> (u64, u64) {
        (64, 32)
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
                    "INSERT INTO resource_usage(key,meter,rule,unit,evidence,limit_value,reset_at) SELECT json_extract(value,'$.key'),json_extract(value,'$.meter'),json_extract(value,'$.rule'),json_extract(value,'$.measure.unit'),json_extract(value,'$.evidence'),json_extract(value,'$.limit'),json_extract(value,'$.window.end') FROM json_each(?1) WHERE 1 ON CONFLICT(key) DO UPDATE SET rule=excluded.rule, unit=excluded.unit, evidence=CASE WHEN resource_usage.evidence IN ('unknown','estimated') THEN resource_usage.evidence ELSE excluded.evidence END, limit_value=excluded.limit_value",
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
            super::run(&self.db,"settle usage","UPDATE resource_attempts SET state=?2,updated_at=unixepoch() WHERE id=?1 AND state IN ('reserved','uncertain')", &[JsValue::from_str(attempt_id),JsValue::from_str(state)]).await
        }))
    }

    fn usage(&self) -> BoxFuture<'_, DomainResult<Vec<BudgetUsage>>> {
        Box::pin(SendFuture::new(query_rows(
            &self.db,
            "resource usage",
            "SELECT key,meter,rule,unit,evidence,consumed,reserved,limit_value AS \"limit\",reset_at FROM resource_usage WHERE reset_at IS NULL OR reset_at>unixepoch() OR reserved>0 ORDER BY rule",
            &[],
        )))
    }
}

impl D1UsageStore {
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
        if !result.success() {
            return Err(DomainError::IndexUnavailable(
                result
                    .error()
                    .unwrap_or_else(|| "usage debit failed".into()),
            ));
        }
        let applied = result
            .results::<serde_json::Value>()
            .map_err(|e| index_err("usage debit result", e))?;
        if applied.len() != charges.len() {
            return Err(self.exhausted(charges).await.err().unwrap_or_else(|| {
                DomainError::Internal("quota transaction applied a partial charge set".into())
            }));
        }
        Ok(())
    }
}
