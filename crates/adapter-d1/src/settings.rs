//! Conditional administration settings, independent of the usage ledger.

use super::{D1Accounting, index_err, query_rows};
use brains_domain::{DomainError, DomainResult, resources::ResourceSettings};
use brains_ports::resources::ResourceSettingsStore;
use futures::future::BoxFuture;
use std::sync::Arc;
use wasm_bindgen::JsValue;
use worker::{D1Database, send::SendFuture};

pub struct D1ResourceSettingsStore {
    db: D1Database,
    accounting: Arc<D1Accounting>,
}

impl D1ResourceSettingsStore {
    pub fn new(db: D1Database) -> Self {
        Self {
            db,
            accounting: Arc::default(),
        }
    }

    pub fn with_accounting(mut self, accounting: Arc<D1Accounting>) -> Self {
        self.accounting = accounting;
        self
    }
}

impl ResourceSettingsStore for D1ResourceSettingsStore {
    fn load(&self) -> BoxFuture<'_, DomainResult<Option<ResourceSettings>>> {
        Box::pin(SendFuture::new(async move {
            #[derive(serde::Deserialize)]
            struct Row {
                revision: String,
                enforced: i64,
                profile: String,
            }
            let rows: Vec<Row> = query_rows(
                &self.db,
                &self.accounting,
                "read resource settings",
                "SELECT revision,enforced,profile FROM resource_settings WHERE id=1",
                &[],
            )
            .await?;
            rows.into_iter()
                .next()
                .map(|r| {
                    let profile = brains_domain::resources::ResourceProfile::parse(&r.profile)?;
                    Ok(ResourceSettings {
                        revision: r.revision,
                        enforced: r.enforced != 0,
                        profile,
                    })
                })
                .transpose()
        }))
    }

    fn save<'a>(
        &'a self,
        previous: Option<&'a str>,
        next: &'a ResourceSettings,
    ) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(async move {
            let profile = serde_json::to_string(&next.profile)
                .map_err(|e| DomainError::Internal(e.to_string()))?;
            let (sql, mut params) = match previous {
                Some(revision) => (
                    "UPDATE resource_settings SET revision=?1,enforced=?2,profile=?3 WHERE id=1 AND revision=?4",
                    vec![JsValue::from_str(revision)],
                ),
                None => (
                    "INSERT INTO resource_settings(id,revision,enforced,profile) VALUES(1,?1,?2,?3) ON CONFLICT(id) DO NOTHING",
                    vec![],
                ),
            };
            let mut values = vec![
                JsValue::from_str(&next.revision),
                JsValue::from_f64(if next.enforced { 1.0 } else { 0.0 }),
                JsValue::from_str(&profile),
            ];
            values.append(&mut params);
            let result = self
                .db
                .prepare(sql)
                .bind(&values)
                .map_err(|e| index_err("write resource settings", e))?
                .run()
                .await
                .map_err(|e| index_err("write resource settings", e))?;
            self.accounting.record(&result)?;
            if !result.success() {
                return Err(DomainError::IndexUnavailable(
                    "resource settings write failed".into(),
                ));
            }
            if result
                .meta()
                .map_err(|e| index_err("settings result", e))?
                .and_then(|m| m.changes)
                .unwrap_or(0)
                != 1
            {
                let actual = self.load().await?.map(|s| s.revision).unwrap_or_default();
                return Err(DomainError::RevisionConflict {
                    path: "resource-settings".into(),
                    current_revision: Some(actual),
                });
            }
            Ok(())
        }))
    }
}
