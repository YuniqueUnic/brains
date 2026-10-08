//! `/maintenance/*` handlers (design §10.5).

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use brainstore_api_types::{ReindexFailure, ReindexPage, ReindexRequest};
use brainstore_domain::DomainError;

use crate::auth::{AuthSource, guard_cookie_mutation};
use crate::error::{ApiError, ApiResult};
use crate::routes::{RequestId, json_response};
use crate::state::{Blob, Head, Idx, Rev, SharedState};

type HandlerState<H, R, B, I> = (
    State<SharedState<H, R, B, I>>,
    Extension<AuthSource>,
    Extension<RequestId>,
);

/// POST /maintenance/reindex — one bounded page, admin-driven. A `validate`
/// route exists for P1 and answers 501 until delivered (design §22.5).
pub async fn reindex<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    headers: HeaderMap,
    Json(body): Json<ReindexRequest>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    if let Some(cookie) = auth.cookie_value() {
        guard_cookie_mutation(
            state.security.as_ref(),
            &headers,
            cookie,
            &state.allowed_origins,
        )
        .map_err(|e| ApiError::new(e, rid))?;
    }
    let limit = match body.limit {
        None => state.limits.reindex_page_max,
        Some(raw) => {
            if raw < 1 || raw > state.limits.reindex_page_max as i64 {
                return Err(ApiError::new(
                    DomainError::InvalidArgument(format!(
                        "limit must be between 1 and {}",
                        state.limits.reindex_page_max
                    )),
                    rid,
                ));
            }
            raw as usize
        }
    };
    let page = state
        .vault
        .reindex_page(auth.principal(), body.cursor.as_deref(), limit)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    let result = ReindexPage {
        processed: page.processed as i64,
        failed: page
            .failed
            .into_iter()
            .map(|f| ReindexFailure {
                path: f.path,
                code: f.code,
            })
            .collect(),
        next_cursor: page.next_cursor,
    };
    Ok(json_response(StatusCode::OK, &result, None))
}

pub async fn validate(Extension(request_id): Extension<RequestId>) -> ApiResult<Response> {
    Err(ApiError::new(
        DomainError::CapabilityUnsupported("server_side_validate"),
        request_id.0.as_str(),
    ))
}
