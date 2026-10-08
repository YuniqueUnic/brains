//! `/revisions`, `/revision` and `/restore` handlers (design §11).

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use std::collections::HashMap;

use brains_api_types::{HistoryPage, RestoreBody, RevisionDto};
use brains_application::Precondition;
use brains_domain::{DomainError, HexId};

use crate::auth::AuthSource;
use crate::error::{ApiError, ApiResult};
use crate::routes::files::{bytes_response, mutation_result};
use crate::routes::{RequestId, json_response, operation_id, parse_required_if_match, query_path};
use crate::state::{Blob, Head, Idx, Rev, SharedState};

type HandlerState<H, R, B, I> = (
    State<SharedState<H, R, B, I>>,
    Extension<AuthSource>,
    Extension<RequestId>,
);

/// GET /revisions — one page of the chain reachable from a captured HEAD.
pub async fn list_revisions<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let path = query_path(&params, rid)?;
    let limit = match params.get("limit") {
        None => state.limits.history_page_max,
        Some(raw) => {
            let value: usize = raw.parse().map_err(|_| {
                ApiError::new(
                    DomainError::InvalidArgument("limit must be an integer".into()),
                    rid,
                )
            })?;
            if value < 1 || value > state.limits.history_page_max {
                return Err(ApiError::new(
                    DomainError::InvalidArgument(format!(
                        "limit must be between 1 and {}",
                        state.limits.history_page_max
                    )),
                    rid,
                ));
            }
            value
        }
    };
    let cursor = params
        .get("cursor")
        .filter(|c| !c.is_empty())
        .map(String::as_str);
    let page = state
        .vault
        .history_page(auth.principal(), &path, cursor, limit)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    let body = HistoryPage {
        items: page
            .items
            .iter()
            .map(|rev| RevisionDto::from_revision(rev, "reachable"))
            .collect(),
        anchor_revision: page.anchor_revision,
        next_cursor: page.next_cursor,
    };
    Ok(json_response(StatusCode::OK, &body, None))
}

/// GET /revision — immutable record by ID; path must match the authorized
/// path. Direct reads are `commit_state: unknown` (design §11.1).
pub async fn get_revision<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let path = query_path(&params, rid)?;
    let revision_id = params
        .get("revision")
        .ok_or_else(|| {
            ApiError::new(
                DomainError::InvalidArgument("missing `revision`".into()),
                rid,
            )
        })
        .and_then(|raw| HexId::from_hex(raw).map_err(|e| ApiError::new(e, rid)))?;
    let record = state
        .vault
        .read_revision(auth.principal(), &path, &revision_id)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    let format = params.get("format").map(String::as_str).unwrap_or("raw");
    if format == "meta" {
        let dto = RevisionDto::from_revision(&record, "unknown");
        return Ok(json_response(
            StatusCode::OK,
            &dto,
            Some(format!("\"{}\"", record.revision.as_str())),
        ));
    }
    let bytes = state
        .vault
        .revision_bytes(&record)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    let media_type = record
        .content
        .as_ref()
        .map(|c| c.media_type.clone())
        .unwrap_or_else(|| "application/octet-stream".into());
    Ok(bytes_response(
        StatusCode::OK,
        &bytes,
        &media_type,
        path.file_name(),
        Some(format!("\"{}\"", record.revision.as_str())),
    ))
}

/// POST /restore — a fresh revision from prior content; HEAD never rewinds.
pub async fn restore<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    headers: HeaderMap,
    Json(body): Json<RestoreBody>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let path = brains_domain::VaultPath::new(&body.path).map_err(|e| ApiError::new(e, rid))?;
    if let Some(cookie) = auth.cookie_value() {
        crate::auth::guard_cookie_mutation(
            state.security.as_ref(),
            &headers,
            cookie,
            &state.allowed_origins,
        )
        .map_err(|e| ApiError::new(e, rid))?;
    }
    let if_match = parse_required_if_match(&headers, rid)?;
    let op_id = operation_id(&headers, rid)?;
    let source = HexId::from_hex(&body.revision).map_err(|e| ApiError::new(e, rid))?;
    let outcome = state
        .vault
        .restore_file(
            auth.principal(),
            &path,
            &source,
            Precondition::IfRevision(if_match),
            auth.principal().actor.clone(),
            op_id,
        )
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    Ok(json_response(
        StatusCode::OK,
        &mutation_result(&outcome),
        Some(format!("\"{}\"", outcome.revision)),
    ))
}
