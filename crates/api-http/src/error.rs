//! The error envelope for statuses that never reach a generated handler:
//! authentication (401) and the CSRF/Origin guard (403) live in middleware,
//! which owns the whole `{"error": ErrorDetail}` table for them (design
//! §11.3, §15). Every status a handler can produce is a typed `raises(...)`
//! exception in `api/brains.idl`, serialized by the generated transport.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Build the shared error envelope: `{"error": {code, message, request_id}}`.
fn envelope(code: &str, message: String, request_id: &str) -> serde_json::Value {
    serde_json::json!({
        "error": {
            "code": code,
            "message": message,
            "request_id": request_id,
        }
    })
}

fn respond(status: StatusCode, code: &str, message: String, request_id: &str) -> Response {
    (status, axum::Json(envelope(code, message, request_id))).into_response()
}

/// 401 for requests without (or with invalid) credentials.
pub fn unauthorized(message: String, request_id: &str) -> Response {
    respond(
        StatusCode::UNAUTHORIZED,
        "UNAUTHENTICATED",
        message,
        request_id,
    )
}

/// 403 for failed CSRF/Origin validation on cookie-driven mutations.
pub fn forbidden(message: String, request_id: &str) -> Response {
    respond(StatusCode::FORBIDDEN, "FORBIDDEN", message, request_id)
}

/// JSON 404 for unknown API routes, never the SPA shell (§5.1).
pub async fn api_not_found() -> Response {
    let request_id = brains_domain::HexId::generate().as_str().to_owned();
    respond(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "no such API route".into(),
        &request_id,
    )
}

/// Liveness probe, identical on both runtimes: no auth, JSON, before the
/// local-profile SPA fallback (which would otherwise swallow the path).
pub async fn healthz() -> Response {
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}
