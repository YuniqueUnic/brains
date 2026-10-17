//! Session lifecycle (IDL `Sessions`): stateless HMAC cookies, CSRF re-derived
//! per request; logout only expires the cookie.

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
        let AuthSource::Bearer(principal) = &ctx.auth else {
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
        let (value, csrf, expires_at) =
            self.state.security.issue_session(principal).map_err(|e| {
                contract::SessionsLoginError::Internal(contract::Internal {
                    error: detail(&e, rid),
                })
            })?;
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

    async fn logout(&self) -> ::xidl_rust_axum::Result<contract::SessionsLogoutResponse> {
        Ok(contract::SessionsLogoutResponse {
            r#return: contract::OkResult { ok: true },
            set_cookie: Some(self.state.session_cookie_policy.cookie_header(
                "",
                0,
                &current().headers,
            )),
        })
    }
}
