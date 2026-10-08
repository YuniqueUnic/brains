//! `/copy`, `/move`, `/diff` handlers (design §8.7, §11.5 — P1).

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use std::collections::HashMap;

use brains_api_types::{DiffResult, TransferBody, TransferResult};
use brains_domain::{DomainError, HexId, VaultPath};

use crate::auth::AuthSource;
use crate::error::{ApiError, ApiResult};
use crate::routes::{RequestId, json_response, query_path};
use crate::state::{Blob, Head, Idx, Rev, SharedState};

type HandlerState<H, R, B, I> = (
    State<SharedState<H, R, B, I>>,
    Extension<AuthSource>,
    Extension<RequestId>,
);

pub async fn copy_file<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    headers: HeaderMap,
    Json(body): Json<TransferBody>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    if let Some(cookie) = auth.cookie_value() {
        crate::auth::guard_cookie_mutation(
            state.security.as_ref(),
            &headers,
            cookie,
            &state.allowed_origins,
        )
        .map_err(|e| ApiError::new(e, rid))?;
    }
    let input = build_transfer(&body, &headers, rid, auth.principal().actor.clone())?;
    let result = state
        .vault
        .copy_file(auth.principal(), input)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    Ok(transfer_response(result))
}

pub async fn move_file<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    headers: HeaderMap,
    Json(body): Json<TransferBody>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    if let Some(cookie) = auth.cookie_value() {
        crate::auth::guard_cookie_mutation(
            state.security.as_ref(),
            &headers,
            cookie,
            &state.allowed_origins,
        )
        .map_err(|e| ApiError::new(e, rid))?;
    }
    let input = build_transfer(&body, &headers, rid, auth.principal().actor.clone())?;
    let result = state
        .vault
        .move_file(auth.principal(), input)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    Ok(transfer_response(result))
}

/// GET /diff — bounded unified diff between two revisions of one path.
pub async fn diff<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let path = query_path(&params, rid)?;
    let from = params
        .get("from")
        .ok_or_else(|| ApiError::new(DomainError::InvalidArgument("missing `from`".into()), rid))
        .and_then(|raw| HexId::from_hex(raw).map_err(|e| ApiError::new(e, rid)))?;
    let to = params
        .get("to")
        .ok_or_else(|| ApiError::new(DomainError::InvalidArgument("missing `to`".into()), rid))
        .and_then(|raw| HexId::from_hex(raw).map_err(|e| ApiError::new(e, rid)))?;
    let diff = state
        .vault
        .diff_revisions(auth.principal(), &path, &from, &to)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    Ok(json_response(StatusCode::OK, &DiffResult { diff }, None))
}

fn build_transfer(
    body: &TransferBody,
    headers: &HeaderMap,
    rid: &str,
    actor: String,
) -> ApiResult<brains_application::transfer::TransferInput> {
    let header_op = crate::routes::single_header(
        headers,
        axum::http::header::HeaderName::from_static("x-operation-id"),
        rid,
    )?;
    // A header/body operation_id mismatch is a programming error (§22.5).
    if let Some(header_op) = header_op
        && header_op != body.operation_id
    {
        return Err(ApiError::new(
            DomainError::InvalidArgument(
                "X-Operation-ID does not match the body operation_id".into(),
            ),
            rid,
        ));
    }
    let source = VaultPath::new(&body.source).map_err(|e| ApiError::new(e, rid))?;
    let target = VaultPath::new(&body.target).map_err(|e| ApiError::new(e, rid))?;
    let expected_source_revision =
        HexId::from_hex(&body.expected_source_revision).map_err(|e| ApiError::new(e, rid))?;
    let operation_id = HexId::from_hex(&body.operation_id).map_err(|e| ApiError::new(e, rid))?;
    Ok(brains_application::transfer::TransferInput {
        source,
        target,
        expected_source_revision,
        actor,
        operation_id,
    })
}

fn transfer_response(result: brains_application::transfer::TransferOutcome) -> Response {
    let body = TransferResult {
        copied: result.copied,
        source_trashed: result.source_trashed,
        partial: result.partial,
        operation_id: result.operation_id,
        target_revision: result.target_revision,
        source_revision: result.source_revision,
        error: result
            .error
            .map(|e| crate::error::ApiError::new(e, "move").body().error),
    };
    let status = if body.partial {
        StatusCode::CONFLICT
    } else {
        StatusCode::OK
    };
    json_response(status, &body, None)
}
