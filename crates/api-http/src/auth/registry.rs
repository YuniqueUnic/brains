//! Shared credential and session rules; runtimes own atomic persistence.

use super::*;

impl RegistryFile {
    fn record(&self, actor: &str) -> DomainResult<&TokenRecord> {
        let id = actor
            .strip_prefix("token:")
            .ok_or(DomainError::Unauthenticated)?;
        self.tokens
            .iter()
            .find(|t| t.enabled && t.id == id)
            .ok_or(DomainError::Unauthenticated)
    }

    fn principal(record: &TokenRecord) -> DomainResult<Principal> {
        Ok(Principal {
            actor: format!("token:{}", record.id),
            scopes: record
                .scopes
                .iter()
                .map(|s| Scope::parse(s))
                .collect::<DomainResult<_>>()?,
        })
    }

    pub fn authenticate(&self, token: &str) -> DomainResult<Principal> {
        let digest = hash_token(token);
        let record = self
            .tokens
            .iter()
            .find(|t| t.enabled && t.token_sha256 == digest)
            .ok_or(DomainError::Unauthenticated)?;
        Self::principal(record)
    }

    pub fn authenticate_session(&self, key: &[u8; 32], cookie: &str) -> DomainResult<Principal> {
        let claims = verify_session_with_key(key, cookie)?;
        let record = self.record(&format!("token:{}", claims.tid))?;
        if claims.epoch != record.session_epoch || self.revoked_sessions.contains_key(&claims.nonce)
        {
            return Err(DomainError::Unauthenticated);
        }
        Self::principal(record)
    }

    pub fn issue_session(
        &self,
        key: &[u8; 32],
        auth: &AuthSource,
    ) -> DomainResult<(String, String, String)> {
        let principal = self.reauthenticate(key, auth)?;
        let record = self.record(&principal.actor)?;
        issue_session_with_key(key, &principal, &record.session_epoch)
    }

    fn reauthenticate(&self, key: &[u8; 32], auth: &AuthSource) -> DomainResult<Principal> {
        // Recheck under the mutation's lock/revision, so a request authenticated
        // before a concurrent rotation cannot mutate the newer credential state.
        let result = match auth {
            AuthSource::Bearer(_, token) => self.authenticate(token),
            AuthSource::Cookie(_, cookie) => self.authenticate_session(key, cookie),
        };
        result.map_err(|_| DomainError::Forbidden("credentials changed; sign in again".into()))
    }

    pub fn rotate_token(
        &mut self,
        key: &[u8; 32],
        auth: &AuthSource,
        current: &str,
        replacement: &str,
    ) -> DomainResult<()> {
        let principal = self.reauthenticate(key, auth)?;
        let verified = self
            .authenticate(current)
            .map_err(|_| DomainError::Forbidden("current token is incorrect".into()))?;
        if verified.actor != principal.actor {
            return Err(DomainError::Forbidden(
                "current token belongs to another principal".into(),
            ));
        }
        if !(32..=4096).contains(&replacement.len())
            || !replacement.bytes().all(|b| (33..=126).contains(&b))
        {
            return Err(DomainError::InvalidArgument(
                "replacement token must be 32..4096 printable ASCII characters without whitespace"
                    .into(),
            ));
        }
        let digest = hash_token(replacement);
        if self.tokens.iter().any(|t| t.token_sha256 == digest) {
            return Err(DomainError::InvalidArgument(
                "replacement token must differ from existing credentials".into(),
            ));
        }
        let id = self.record(&principal.actor)?.id.clone();
        let record = self
            .tokens
            .iter_mut()
            .find(|t| t.id == id)
            .expect("verified record");
        record.token_sha256 = digest;
        record.plaintext = None;
        record.session_epoch = brains_domain::HexId::generate().as_str().to_owned();
        self.revoked_sessions.retain(|_, exp| *exp > now_unix());
        Ok(())
    }

    pub fn revoke_sessions(
        &mut self,
        key: &[u8; 32],
        auth: &AuthSource,
        all: bool,
    ) -> DomainResult<()> {
        let principal = self.reauthenticate(key, auth)?;
        if all {
            let id = self.record(&principal.actor)?.id.clone();
            self.tokens
                .iter_mut()
                .find(|t| t.id == id)
                .expect("verified record")
                .session_epoch = brains_domain::HexId::generate().as_str().to_owned();
        } else if let AuthSource::Cookie(_, cookie) = auth {
            let claims = verify_session_with_key(key, cookie)?;
            self.revoked_sessions.insert(claims.nonce, claims.exp);
        }
        self.revoked_sessions.retain(|_, exp| *exp > now_unix());
        Ok(())
    }
}
