//! Bounded, administrative archive operations shared by native and Worker.

use crate::{
    auth::{AuthSource, guard_cookie_mutation},
    error::{ApiError, ApiResult},
    routes::{RequestId, json_response, query_path},
    state::{Blob, Head, Idx, Rev, SharedState},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use brains_domain::{DomainError, HexId, Revision, VaultPath};
use std::collections::HashMap;

type HandlerState<H, R, B, I> = (
    State<SharedState<H, R, B, I>>,
    Extension<AuthSource>,
    Extension<RequestId>,
);

fn identity(params: &HashMap<String, String>, rid: &str) -> ApiResult<(VaultPath, HexId)> {
    let path = query_path(params, rid)?;
    let id = params.get("revision").ok_or_else(|| {
        ApiError::new(
            DomainError::InvalidArgument("revision is required".into()),
            rid,
        )
    })?;
    Ok((
        path,
        HexId::from_hex(id).map_err(|e| ApiError::new(e, rid))?,
    ))
}

fn guard<H: Head, R: Rev, B: Blob, I: Idx>(
    state: &SharedState<H, R, B, I>,
    auth: &AuthSource,
    headers: &HeaderMap,
    rid: &str,
) -> ApiResult<()> {
    if let Some(cookie) = auth.cookie_value() {
        guard_cookie_mutation(
            state.security.as_ref(),
            headers,
            cookie,
            &state.allowed_origins,
        )
        .map_err(|e| ApiError::new(e, rid))?;
    }
    Ok(())
}

pub async fn get_revision<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let (path, id) = identity(&params, rid)?;
    let revision = state
        .vault
        .archive_revision(auth.principal(), &path, &id)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    Ok(json_response(StatusCode::OK, &revision, None))
}

pub async fn put_revision<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    headers: HeaderMap,
    Json(revision): Json<Revision>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    guard(&state, &auth, &headers, rid)?;
    let revision = state
        .vault
        .import_revision(auth.principal(), &revision)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    Ok(json_response(StatusCode::OK, &revision, None))
}

pub async fn put_blob<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    guard(&state, &auth, &headers, rid)?;
    let (path, id) = identity(&params, rid)?;
    state
        .vault
        .import_blob(auth.principal(), &path, &id, &body)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    Ok(json_response(
        StatusCode::OK,
        &serde_json::json!({"stored":true}),
        None,
    ))
}

pub async fn put_head<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    guard(&state, &auth, &headers, rid)?;
    let (path, id) = identity(&params, rid)?;
    let head = state
        .vault
        .import_head(auth.principal(), &path, &id)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    Ok(json_response(
        StatusCode::OK,
        &serde_json::json!({"head":head,"index_state":"pending"}),
        None,
    ))
}
