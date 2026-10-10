//! `/session` handlers (design §15.1): bearer ⇄ signed HttpOnly cookie.

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use brains_api_types::Session;
use brains_domain::DomainError;

use crate::auth::{AuthSource, guard_cookie_mutation};
use crate::error::{ApiError, ApiResult};
use crate::routes::{RequestId, json_response};
use crate::state::{Blob, Head, Idx, Rev, SharedState};

type HandlerState<H, R, B, I> = (
    State<SharedState<H, R, B, I>>,
    Option<Extension<AuthSource>>,
    Extension<RequestId>,
);

fn principal_or_error(
    auth: &Option<Extension<AuthSource>>,
    rid: &str,
) -> Result<AuthSource, ApiError> {
    auth.as_ref()
        .map(|ext| ext.0.clone())
        .ok_or_else(|| ApiError::new(DomainError::Unauthenticated, rid))
}

fn session_body(principal: &brains_domain::Principal, csrf: Option<String>) -> Session {
    Session {
        actor: principal.actor.clone(),
        scopes: principal.scopes.iter().map(|s| s.as_string()).collect(),
        expires_at: None,
        csrf_token: csrf,
    }
}

/// POST /session — exchange a Bearer token for a session cookie. Origin is
/// validated whenever a browser sends one (design §15.2); plain HTTP
/// clients without an Origin are not CSRF material.
pub async fn create_session<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let auth = principal_or_error(&auth, rid)?;
    let principal = match &auth {
        AuthSource::Bearer(principal) => principal,
        AuthSource::Cookie(..) => {
            return Err(ApiError::new(
                DomainError::InvalidArgument("already authenticated via session cookie".into()),
                rid,
            ));
        }
    };
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
        && !crate::auth::origin_allowed(origin, &headers, &state.allowed_origins)
    {
        return Err(ApiError::new(
            DomainError::Forbidden(format!("origin {origin} is not allowed")),
            rid,
        ));
    }
    let (value, csrf, expires_at) = state
        .security
        .issue_session(principal)
        .map_err(|e| ApiError::new(e, rid))?;
    let cookie = state
        .session_cookie_policy
        .cookie_header(&value, 8 * 3600, &headers);
    let mut body = session_body(principal, Some(csrf));
    body.expires_at = Some(expires_at);
    let mut response = json_response(StatusCode::OK, &body, None);
    if let Ok(value) = header::HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    Ok(response)
}

/// GET /session — report the current principal.
pub async fn get_session<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let auth = principal_or_error(&auth, rid)?;
    let csrf = auth
        .cookie_value()
        .map(|value| state.security.csrf_for(value));
    let body = session_body(auth.principal(), csrf);
    Ok(json_response(StatusCode::OK, &body, None))
}

/// DELETE /session — clear the browser cookie.
pub async fn delete_session<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let auth = principal_or_error(&auth, rid)?;
    if let Some(cookie) = auth.cookie_value() {
        guard_cookie_mutation(
            state.security.as_ref(),
            &headers,
            cookie,
            &state.allowed_origins,
        )
        .map_err(|e| ApiError::new(e, rid))?;
    }
    let clear = state.session_cookie_policy.cookie_header("", 0, &headers);
    let mut response = Json(brains_api_types::OkResult { ok: true }).into_response();
    if let Ok(value) = header::HeaderValue::from_str(&clear) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    Ok(response)
}
