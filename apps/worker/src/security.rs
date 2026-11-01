//! D1-backed active credentials; deployment secrets are bootstrap inputs only.

use std::sync::Arc;

use base64::Engine;
use brains_adapter_d1::D1Accounting;
use brains_api_http::auth::{AuthSource, RegistryFile, Security, sign_with_key};
use brains_domain::{DomainError, DomainResult, Principal};
use futures::future::BoxFuture;
use wasm_bindgen::JsValue;
use worker::{D1Database, send::SendFuture};

pub struct EnvSecurity {
    registry: RegistryFile,
    revision: Option<String>,
    session_key: [u8; 32],
    db: D1Database,
    accounting: Arc<D1Accounting>,
}

fn unavailable() -> DomainError {
    DomainError::StorageUnavailable(
        "security state unavailable; check D1 and its migrations".into(),
    )
}

impl EnvSecurity {
    pub async fn load(
        bootstrap: &str,
        session_key: [u8; 32],
        db: D1Database,
        accounting: Arc<D1Accounting>,
    ) -> DomainResult<Self> {
        #[derive(serde::Deserialize)]
        struct Row {
            revision: String,
            registry: String,
        }
        let observed_at = time::OffsetDateTime::now_utc().unix_timestamp();
        let result = db
            .prepare("SELECT revision,registry FROM security_state WHERE id=1")
            .all()
            .await
            .map_err(|_| {
                accounting.unmeasured(observed_at);
                unavailable()
            })?;
        accounting.record(&result, observed_at)?;
        if !result.success() {
            return Err(unavailable());
        }
        let row = result
            .results::<Row>()
            .map_err(|_| unavailable())?
            .into_iter()
            .next();
        let (revision, raw) = match &row {
            Some(row) => (Some(row.revision.clone()), row.registry.as_str()),
            None => (None, bootstrap),
        };
        let mut registry: RegistryFile = serde_json::from_str(raw)
            .map_err(|_| DomainError::Internal("invalid security registry".into()))?;
        if row.is_some() {
            let bootstrap: RegistryFile = serde_json::from_str(bootstrap)
                .map_err(|_| DomainError::Internal("invalid bootstrap registry".into()))?;
            for record in bootstrap.tokens {
                if !registry.tokens.iter().any(|t| t.id == record.id) {
                    registry.tokens.push(record);
                }
            }
        }
        for record in &mut registry.tokens {
            record.plaintext = None;
        }
        Ok(Self {
            registry,
            revision,
            session_key,
            db,
            accounting,
        })
    }

    async fn save(&self, registry: &RegistryFile) -> DomainResult<()> {
        let revision = brains_domain::HexId::generate().as_str().to_owned();
        let raw = serde_json::to_string(registry)
            .map_err(|_| DomainError::Internal("cannot encode security state".into()))?;
        let mut params = vec![JsValue::from_str(&revision), JsValue::from_str(&raw)];
        let sql = if let Some(previous) = &self.revision {
            params.push(JsValue::from_str(previous));
            "UPDATE security_state SET revision=?1,registry=?2 WHERE id=1 AND revision=?3"
        } else {
            "INSERT INTO security_state(id,revision,registry) VALUES(1,?1,?2) ON CONFLICT(id) DO NOTHING"
        };
        let observed_at = time::OffsetDateTime::now_utc().unix_timestamp();
        let result = self
            .db
            .prepare(sql)
            .bind(&params)
            .map_err(|_| unavailable())?
            .run()
            .await
            .map_err(|_| {
                self.accounting.unmeasured(observed_at);
                unavailable()
            })?;
        self.accounting.record(&result, observed_at)?;
        if !result.success() {
            return Err(unavailable());
        }
        if result
            .meta()
            .map_err(|_| unavailable())?
            .and_then(|m| m.changes)
            != Some(1)
        {
            return Err(DomainError::RevisionConflict {
                path: "security-state".into(),
                current_revision: None,
            });
        }
        Ok(())
    }
}

impl Security for EnvSecurity {
    fn authenticate(&self, token: &str) -> DomainResult<Principal> {
        self.registry.authenticate(token)
    }
    fn authenticate_session(&self, cookie: &str) -> DomainResult<Principal> {
        self.registry
            .authenticate_session(&self.session_key, cookie)
    }
    fn issue_session(&self, auth: &AuthSource) -> DomainResult<(String, String, String)> {
        self.registry.issue_session(&self.session_key, auth)
    }
    fn csrf_for(&self, cookie: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sign_with_key(
            &self.session_key,
            format!("csrf:{cookie}").as_bytes(),
        ))
    }
    fn rotate_token<'a>(
        &'a self,
        auth: &'a AuthSource,
        current: &'a str,
        replacement: &'a str,
    ) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(async move {
            let mut next = self.registry.clone();
            next.rotate_token(&self.session_key, auth, current, replacement)?;
            self.save(&next).await
        }))
    }
    fn revoke_sessions<'a>(
        &'a self,
        auth: &'a AuthSource,
        all: bool,
    ) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(async move {
            let mut next = self.registry.clone();
            next.revoke_sessions(&self.session_key, auth, all)?;
            self.save(&next).await
        }))
    }
}

/// Decode a 64-hex-char secret var into a 32-byte key.
pub fn hex_key(raw: &str, name: &str) -> DomainResult<[u8; 32]> {
    hex::decode(raw)
        .map_err(|_| DomainError::Internal(format!("{name} must be exactly 64 hex chars")))?
        .try_into()
        .map_err(|_| DomainError::Internal(format!("{name} must be exactly 64 hex chars")))
}
