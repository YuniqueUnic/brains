//! `/search` and `/links` handlers (design §10.3, §11.1).

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use std::collections::HashMap;

use brainstore_api_types::{LinkItem, LinkPage, SearchHit, SearchPage};
use brainstore_application::search::{SearchMode, SearchQuery};
use brainstore_domain::DomainError;

use crate::auth::AuthSource;
use crate::error::{ApiError, ApiResult};
use crate::routes::{RequestId, json_response, query_path};
use crate::state::{Blob, Head, Idx, Rev, SharedState};

type HandlerState<H, R, B, I> = (
    State<SharedState<H, R, B, I>>,
    Extension<AuthSource>,
    Extension<RequestId>,
);

/// GET /search — bounded literal windows or explicit FTS.
pub async fn search<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let q = params
        .get("q")
        .filter(|q| !q.is_empty())
        .ok_or_else(|| ApiError::new(DomainError::InvalidArgument("missing `q`".into()), rid))?;
    if q.len() > 512 {
        return Err(ApiError::new(
            DomainError::InvalidArgument("q must be at most 512 bytes".into()),
            rid,
        ));
    }
    let mode = match params.get("mode").map(String::as_str).unwrap_or("literal") {
        "literal" => SearchMode::Literal,
        "fts" => SearchMode::Fts,
        other => {
            return Err(ApiError::new(
                DomainError::InvalidArgument(format!("unknown search mode `{other}`")),
                rid,
            ));
        }
    };
    let scan_limit = match params.get("scan_limit") {
        None => state.limits.scan_window_max,
        Some(raw) => {
            let value: usize = raw.parse().map_err(|_| {
                ApiError::new(
                    DomainError::InvalidArgument("scan_limit must be an integer".into()),
                    rid,
                )
            })?;
            if value < 1 || value > state.limits.scan_window_max {
                return Err(ApiError::new(
                    DomainError::InvalidArgument(format!(
                        "scan_limit must be between 1 and {}",
                        state.limits.scan_window_max
                    )),
                    rid,
                ));
            }
            value
        }
    };
    let query = SearchQuery {
        q: q.clone(),
        mode,
        prefix: params.get("prefix").filter(|p| !p.is_empty()).cloned(),
        tag: params.get("tag").filter(|t| !t.is_empty()).cloned(),
        cursor: params.get("cursor").filter(|c| !c.is_empty()).cloned(),
        scan_limit,
    };
    let result = state
        .vault
        .search(auth.principal(), query)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    let body = SearchPage {
        items: result
            .items
            .into_iter()
            .map(|hit| SearchHit {
                path: hit.path,
                indexed_revision: hit.indexed_revision,
                snippet: hit.snippet,
            })
            .collect(),
        next_cursor: result.next_cursor,
        complete: result.complete,
        scanned: result.scanned as i64,
        index_state: if result.building {
            "building"
        } else {
            "available"
        }
        .into(),
    };
    Ok(json_response(StatusCode::OK, &body, None))
}

/// GET /links — outbound or reverse links from the derived index.
pub async fn links<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let path = query_path(&params, rid)?;
    let backlinks = match params
        .get("direction")
        .map(String::as_str)
        .unwrap_or("outbound")
    {
        "outbound" => false,
        "backlinks" => true,
        other => {
            return Err(ApiError::new(
                DomainError::InvalidArgument(format!("unknown direction `{other}`")),
                rid,
            ));
        }
    };
    let cursor = params
        .get("cursor")
        .filter(|c| !c.is_empty())
        .map(String::as_str);
    let page = state
        .vault
        .links(auth.principal(), &path, backlinks, cursor, 50)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    let body = LinkPage {
        items: page
            .rows
            .into_iter()
            .map(|row| LinkItem {
                source: row.source,
                target: row.target,
                kind: row.kind,
                source_revision: Some(row.source_revision),
            })
            .collect(),
        next_cursor: page.next_cursor,
    };
    Ok(json_response(StatusCode::OK, &body, None))
}
