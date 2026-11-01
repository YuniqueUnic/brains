//! Session lifecycle (IDL `Sessions`): stateless HMAC cookies, CSRF re-derived
//! per request; revocation is persisted before the cookie is cleared.

use brains_domain::DomainError;

use crate::auth::AuthSource;
use crate::context::current;
use crate::contract::brains as contract;
use crate::state::{Blob, Head, Idx, Rev, SharedState};

use super::error::detail;

pub struct SessionService<H: Head, R: Rev, B: Blob, I: Idx> {
    pub state: SharedState<H, R, B, I>,
}

impl<H: Head, R: Rev, B: Blob, I: Idx> Clone for SessionService<H, R, B, I> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

#[async_trait::async_trait]
impl<H: Head, R: Rev, B: Blob, I: Idx> contract::Sessions for SessionService<H, R, B, I> {
    async fn login(
        &self,
        _authorization: String,
    ) -> Result<contract::SessionsLoginResponse, contract::SessionsLoginError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        // The middleware only lets Bearer credentials through to this point;
        // a session cookie is a typed 400 (already authenticated).
        let AuthSource::Bearer(principal, _) = &ctx.auth else {
            return Err(contract::SessionsLoginError::BadRequest(
                contract::BadRequest {
                    error: detail(
                        &DomainError::InvalidArgument(
                            "already authenticated via session cookie".into(),
                        ),
                        rid,
                    ),
                },
            ));
        };
        let (value, csrf, expires_at) = self
            .state
            .security
            .issue_session(&ctx.auth)
            .map_err(|e| super::error::login_error(e, rid))?;
        let cookie = self
            .state
            .session_cookie_policy
            .cookie_header(&value, 8 * 3600, &ctx.headers);
        Ok(contract::SessionsLoginResponse {
            r#return: contract::Session {
                actor: principal.actor.clone(),
                scopes: principal.scopes.iter().map(|s| s.as_string()).collect(),
                expires_at: Some(expires_at),
                csrf_token: Some(csrf),
            },
            set_cookie: Some(cookie),
        })
    }

    async fn current(&self) -> ::xidl_rust_axum::Result<contract::Session> {
        let ctx = current();
        let csrf = ctx
            .auth
            .cookie_value()
            .map(|value| self.state.security.csrf_for(value));
        Ok(contract::Session {
            actor: ctx.auth.principal().actor.clone(),
            scopes: ctx
                .auth
                .principal()
                .scopes
                .iter()
                .map(|s| s.as_string())
                .collect(),
            expires_at: None,
            csrf_token: csrf,
        })
    }

    async fn logout(
        &self,
    ) -> Result<contract::SessionsLogoutResponse, contract::SessionsLogoutError> {
        let ctx = current();
        self.state
            .security
            .revoke_sessions(&ctx.auth, false)
            .await
            .map_err(|e| super::error::logout_error(e, ctx.request_id.as_str()))?;
        Ok(contract::SessionsLogoutResponse {
            r#return: contract::OkResult { ok: true },
            set_cookie: Some(self.state.session_cookie_policy.cookie_header(
                "",
                0,
                &current().headers,
            )),
        })
    }
    async fn revoke_all(
        &self,
    ) -> Result<contract::SessionsRevokeAllResponse, contract::SessionsRevokeAllError> {
        let ctx = current();
        self.state
            .security
            .revoke_sessions(&ctx.auth, true)
            .await
            .map_err(|e| super::error::revoke_sessions_error(e, ctx.request_id.as_str()))?;
        Ok(contract::SessionsRevokeAllResponse {
            r#return: contract::OkResult { ok: true },
            set_cookie: Some(
                self.state
                    .session_cookie_policy
                    .cookie_header("", 0, &ctx.headers),
            ),
        })
    }

    async fn rotate_token(
        &self,
        input: contract::TokenRotationInput,
    ) -> Result<contract::SessionsRotateTokenResponse, contract::SessionsRotateTokenError> {
        let ctx = current();
        self.state
            .security
            .rotate_token(&ctx.auth, &input.current_token, &input.replacement_token)
            .await
            .map_err(|e| super::error::rotate_token_error(e, ctx.request_id.as_str()))?;
        Ok(contract::SessionsRotateTokenResponse {
            r#return: contract::OkResult { ok: true },
            set_cookie: Some(
                self.state
                    .session_cookie_policy
                    .cookie_header("", 0, &ctx.headers),
            ),
        })
    }
}
