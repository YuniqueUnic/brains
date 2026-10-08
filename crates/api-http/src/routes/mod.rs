//! Router assembly and shared handler plumbing: request IDs, principal
//! extraction, precondition parsing, ETag helpers.

pub mod files;
pub mod history;
pub mod maintenance;
pub mod search;
pub mod session;
pub mod transfer;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};

use brains_application::Precondition;
use brains_domain::{DomainError, HexId, VaultPath};

use crate::auth::AuthSource;
use crate::error::{ApiError, ApiResult};
use crate::state::{Blob, Head, Idx, Rev, SharedState};

#[derive(Clone)]
pub struct RequestId(pub String);

pub async fn api_not_found() -> Response {
    use brains_api_types::{ErrorBody, ErrorDetail};
    (
        StatusCode::NOT_FOUND,
        axum::Json(ErrorBody {
            error: ErrorDetail {
                code: "NOT_FOUND".into(),
                message: "no such API route".into(),
                request_id: HexId::generate().as_str().to_owned(),
                path: None,
                current_revision: None,
                details: None,
            },
        }),
    )
        .into_response()
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

pub fn build_router<H: Head, R: Rev, B: Blob, I: Idx>(state: SharedState<H, R, B, I>) -> Router {
    let api = Router::new()
        .route("/capabilities", get(files::capabilities))
        .route("/files", get(files::list_files))
        .route(
            "/file",
            get(files::get_file)
                .head(files::head_file)
                .put(files::put_file)
                .delete(files::delete_file)
                .patch(files::patch_file),
        )
        .route("/revisions", get(history::list_revisions))
        .route("/revision", get(history::get_revision))
        .route("/restore", post(history::restore))
        .route("/search", get(search::search))
        .route("/links", get(search::links))
        .route("/maintenance/reindex", post(maintenance::reindex))
        .route("/maintenance/validate", post(maintenance::validate))
        .route(
            "/session",
            get(session::get_session)
                .post(session::create_session)
                .delete(session::delete_session),
        )
        .route("/copy", post(transfer::copy_file))
        .route("/move", post(transfer::move_file))
        .route("/diff", get(transfer::diff))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ))
        .with_state(state.clone())
        // Unknown /api/v1 routes answer JSON, never the SPA shell (§5.1).
        .fallback(api_not_found)
        // One body-limit owner for both runtimes (§6.1): the largest payload
        // plus JSON envelope headroom, instead of a per-app magic constant.
        .layer(axum::extract::DefaultBodyLimit::max(
            state.limits.max_attachment_bytes + 1024 * 1024,
        ));

    // `mut` is consumed by the local-profile SPA fallback below.
    #[allow(unused_mut)]
    let mut router = Router::new()
        .nest("/api/v1", api)
        // Anything under /api but outside /api/v1 is an API miss: JSON 404,
        // never the SPA shell (§5.1).
        .route("/api", any(api_not_found))
        .route("/api/{*rest}", any(api_not_found))
        .route("/healthz", get(healthz))
        .with_state(state.clone());
    // SPA fallback is local-profile only (§9.2); the Worker serves assets via
    // the ASSETS binding and never sees non-API paths.
    #[cfg(feature = "local")]
    if let Some(dist) = &state.web_dist {
        let index = dist.join("index.html");
        let serve = tower_http::services::ServeDir::new(dist)
            .append_index_html_on_directories(true)
            .fallback(tower_http::services::ServeFile::new(index));
        router = router.fallback_service(serve);
    }
    #[cfg(not(feature = "local"))]
    let _ = &state.web_dist;
    router
}

/// Every /api/v1 request carries a request id and, except session creation,
/// an authenticated principal (design §15.1).
async fn authenticate<H: Head, R: Rev, B: Blob, I: Idx>(
    State(state): State<SharedState<H, R, B, I>>,
    mut request: Request,
    next: Next,
) -> Response {
    let request_id = HexId::generate().as_str().to_owned();
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));
    let source = AuthSource::from_request(state.security.as_ref(), request.headers());
    // The nest strips the /api/v1 prefix before this middleware runs.
    let session_create = request.method() == Method::POST && request.uri().path() == "/session";
    let mut response = match source {
        Ok(source) => {
            request.extensions_mut().insert(source);
            next.run(request).await
        }
        Err(error) => {
            if session_create {
                // The session handler itself enforces the Bearer requirement.
                next.run(request).await
            } else {
                ApiError::new(error, request_id).into_response()
            }
        }
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

// ---------------------------------------------------------------------------
// Shared extraction helpers
// ---------------------------------------------------------------------------

pub fn query_path(
    params: &std::collections::HashMap<String, String>,
    rid: &str,
) -> ApiResult<VaultPath> {
    let raw = params
        .get("path")
        .ok_or_else(|| ApiError::new(DomainError::InvalidArgument("missing `path`".into()), rid))?;
    VaultPath::new(raw).map_err(|e| ApiError::new(e, rid))
}

/// Exactly one write precondition, strictly formed (design §8.1): weak
/// ETags, wildcards for updates, multi-values and absent conditions all fail.
pub fn parse_write_precondition(headers: &HeaderMap, rid: &str) -> ApiResult<Precondition> {
    let if_match = single_header(headers, header::IF_MATCH, rid)?;
    let if_none_match = single_header(headers, header::IF_NONE_MATCH, rid)?;
    match (if_match, if_none_match) {
        (Some(_), Some(_)) => Err(invalid(
            "provide exactly one of If-Match or If-None-Match",
            rid,
        )),
        (Some(if_match), None) => {
            let revision = parse_strong_etag(&if_match).ok_or_else(|| {
                invalid(
                    "If-Match must be one strong ETag for the base revision",
                    rid,
                )
            })?;
            Ok(Precondition::IfRevision(revision))
        }
        (None, Some(star)) if star == "*" => Ok(Precondition::Create),
        (None, Some(_)) => Err(invalid("If-None-Match for writes only supports `*`", rid)),
        (None, None) => Err(ApiError::new(DomainError::PreconditionRequired, rid)),
    }
}

/// A required If-Match (DELETE / restore / patch): absence is 428.
pub fn parse_required_if_match(headers: &HeaderMap, rid: &str) -> ApiResult<HexId> {
    match single_header(headers, header::IF_MATCH, rid)? {
        Some(value) => parse_strong_etag(&value).ok_or_else(|| {
            invalid(
                "If-Match must be one strong ETag for the base revision",
                rid,
            )
        }),
        None => Err(ApiError::new(DomainError::PreconditionRequired, rid)),
    }
}

pub fn parse_strong_etag(value: &str) -> Option<HexId> {
    let trimmed = value.trim();
    if trimmed.starts_with("W/") {
        return None;
    }
    let inner = trimmed.strip_prefix('"')?.strip_suffix('"')?;
    if inner == "*" {
        return None;
    }
    HexId::from_hex(inner).ok()
}

fn invalid(message: &str, rid: &str) -> ApiError {
    ApiError::new(DomainError::InvalidArgument(message.to_owned()), rid)
}

/// Single-valued header read; comma-joined values are rejected so a client
/// cannot smuggle a condition list past the parser.
pub fn single_header(
    headers: &HeaderMap,
    name: header::HeaderName,
    rid: &str,
) -> ApiResult<Option<String>> {
    let name_display = name.as_str().to_owned();
    let mut values = headers.get_all(name).iter();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(ApiError::new(
            DomainError::InvalidArgument(format!(
                "multiple `{name_display}` values are not allowed"
            )),
            rid,
        ));
    }
    let value = first
        .to_str()
        .map_err(|_| ApiError::new(DomainError::InvalidArgument("non-ASCII header".into()), rid))?;
    if value.contains(',') {
        return Err(ApiError::new(
            DomainError::InvalidArgument(format!("`{name_display}` must be a single value")),
            rid,
        ));
    }
    Ok(Some(value.to_owned()))
}

pub fn operation_id(headers: &HeaderMap, rid: &str) -> ApiResult<HexId> {
    match single_header(
        headers,
        header::HeaderName::from_static("x-operation-id"),
        rid,
    )? {
        None => Ok(HexId::generate()),
        Some(raw) => HexId::from_hex(&raw).map_err(|e| ApiError::new(e, rid)),
    }
}

/// GET/HEAD freshness check: `If-None-Match` matching the current ETag → 304
/// (RFC 9110 §13.1.2: weak comparison, `*`, repeated header lines all match).
pub fn not_modified(headers: &HeaderMap, etag: &str) -> bool {
    let current = etag.trim().trim_start_matches("W/").trim_matches('"');
    headers.get_all(header::IF_NONE_MATCH).iter().any(|value| {
        let Ok(values) = value.to_str() else {
            return false;
        };
        values.split(',').map(str::trim).any(|candidate| {
            let candidate = candidate.trim_start_matches("W/").trim_matches('"');
            candidate == "*" || candidate == current
        })
    })
}

/// Content-Disposition policy (design §13.3, §15.3): P0 forces HTML/SVG and
/// PDF to download; everything else may render inline.
pub fn content_disposition(media_type: &str, file_name: &str) -> Option<String> {
    let force_download = media_type.starts_with("text/html")
        || media_type.starts_with("image/svg+xml")
        || media_type == "application/pdf";
    if !force_download {
        return None;
    }
    let safe_name: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Some(format!("attachment; filename=\"{safe_name}\""))
}

pub fn json_response<T: serde::Serialize>(
    status: StatusCode,
    value: &T,
    etag: Option<String>,
) -> Response {
    let mut response = (status, axum::Json(value)).into_response();
    if let Some(etag) = etag
        && let Ok(value) = HeaderValue::from_str(&etag)
    {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}
