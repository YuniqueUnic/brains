//! Worker security backend (design §15.1): the token registry and signing
//! keys come from Worker vars/secrets instead of the local `auth.json`, and
//! reuse the exact same hashing, session-cookie and CSRF primitives as the
//! file-backed store via `brains_api_http::auth`.

use base64::Engine;
use brains_api_http::auth::{
    Security, SessionClaims, TokenRecord, hash_token, issue_session_with_key, sign_with_key,
    verify_session_with_key,
};

use brains_domain::{DomainError, DomainResult, Principal, Scope};

pub struct EnvSecurity {
    registry: Vec<TokenRecord>,
    session_key: [u8; 32],
}

impl EnvSecurity {
    /// `TOKEN_REGISTRY_JSON`: `{"tokens":[{"id","name","token_sha256","scopes","enabled"}]}`.
    /// Tokens live only as SHA-256 hashes; plaintext never reaches the Worker.
    pub fn from_registry_json(registry_json: &str, session_key: [u8; 32]) -> DomainResult<Self> {
        #[derive(serde::Deserialize)]
        struct Registry {
            #[serde(default)]
            tokens: Vec<TokenRecord>,
        }
        let registry: Registry = serde_json::from_str(registry_json)
            .map_err(|e| DomainError::Internal(format!("TOKEN_REGISTRY_JSON malformed: {e}")))?;
        Ok(Self {
            registry: registry.tokens,
            session_key,
        })
    }
}

impl Security for EnvSecurity {
    fn authenticate(&self, token: &str) -> DomainResult<Principal> {
        let digest = hash_token(token);
        let record = self
            .registry
            .iter()
            .find(|t| t.enabled && t.token_sha256 == digest)
            .ok_or(DomainError::Unauthenticated)?;
        let scopes = record
            .scopes
            .iter()
            .map(|s| Scope::parse(s))
            .collect::<DomainResult<Vec<_>>>()?;
        Ok(Principal {
            actor: format!("token:{}", record.id),
            scopes,
        })
    }

    fn authenticate_session(&self, cookie_value: &str) -> DomainResult<Principal> {
        let claims: SessionClaims = verify_session_with_key(&self.session_key, cookie_value)?;
        let record = self
            .registry
            .iter()
            .find(|t| t.id == claims.tid && t.enabled)
            .ok_or(DomainError::Unauthenticated)?;
        let scopes = record
            .scopes
            .iter()
            .map(|s| Scope::parse(s))
            .collect::<DomainResult<Vec<_>>>()?;
        Ok(Principal {
            actor: format!("token:{}", record.id),
            scopes,
        })
    }

    fn issue_session(&self, principal: &Principal) -> DomainResult<(String, String, String)> {
        issue_session_with_key(&self.session_key, principal)
    }

    fn csrf_for(&self, cookie_value: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sign_with_key(
            &self.session_key,
            format!("csrf:{cookie_value}").as_bytes(),
        ))
    }
}

/// Decode a 64-hex-char secret var into a 32-byte key.
pub fn hex_key(raw: &str, name: &str) -> DomainResult<[u8; 32]> {
    let bytes = hex_decode(raw, name)?;
    bytes.try_into().map_err(|_| {
        DomainError::Internal(format!("{name} must be exactly 64 hex chars (32 bytes)"))
    })
}

fn hex_decode(raw: &str, name: &str) -> DomainResult<Vec<u8>> {
    hex::decode(raw).map_err(|e| DomainError::Internal(format!("{name}: {e}")))
}
