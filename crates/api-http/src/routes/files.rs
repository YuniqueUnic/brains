//! `/capabilities`, `/files` and `/file` handlers (design §11).

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use std::collections::HashMap;

use brains_api_types::{Entry, FilePage, MutationResult, PatchBody, PatchDryRun, PurgeResult};
use brains_application::Precondition;
use brains_application::commit::CommitInput;
use brains_domain::{DomainError, HeadState};
use sha2::{Digest, Sha256};

use crate::auth::{AuthSource, guard_cookie_mutation};
use crate::error::{ApiError, ApiResult};
use crate::routes::{
    RequestId, content_disposition, json_response, not_modified, operation_id,
    parse_required_if_match, parse_write_precondition, query_path,
};
use crate::state::{Blob, Head, Idx, Rev, SharedState};

type HandlerState<H, R, B, I> = (
    State<SharedState<H, R, B, I>>,
    Extension<AuthSource>,
    Extension<RequestId>,
);

pub async fn capabilities<H: Head, R: Rev, B: Blob, I: Idx>(
    State(state): State<SharedState<H, R, B, I>>,
    Extension(_auth): Extension<AuthSource>,
) -> ApiResult<Response> {
    Ok(json_response(StatusCode::OK, &state.capabilities, None))
}

/// GET /files — authoritative or index-backed listing with pagination.
pub async fn list_files<H: Head, R: Rev, B: Blob, I: Idx>(
    State(state): State<SharedState<H, R, B, I>>,
    Extension(auth): Extension<AuthSource>,
    Extension(request_id): Extension<RequestId>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Response> {
    let principal = auth.principal();
    let source = params.get("source").map(String::as_str).unwrap_or("store");
    let tag = params.get("tag").filter(|t| !t.is_empty()).cloned();
    let kind = params
        .get("kind")
        .map(String::as_str)
        .and_then(brains_domain::FileKind::parse);
    let sort = params.get("sort").map(String::as_str).unwrap_or("path");
    let include_deleted = params
        .get("include_deleted")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    let limit = clamp_limit(
        params.get("limit"),
        state.limits.list_page_default,
        state.limits.list_page_max,
    )
    .map_err(|e| ApiError::new(e, request_id.0.as_str()))?;
    let cursor = params.get("cursor").filter(|c| !c.is_empty()).cloned();
    let prefix = params.get("prefix").filter(|p| !p.is_empty()).cloned();

    if source == "store" {
        // Index-only filters must fail loudly, never be ignored (design §11.1).
        if tag.is_some() || kind.is_some() || sort == "updated_desc" {
            return Err(ApiError::new(
                DomainError::InvalidArgument(
                    "tag/kind/sort=updated_desc require source=index".into(),
                ),
                request_id.0.as_str(),
            ));
        }
        let page = state
            .vault
            .list_from_store(
                principal,
                prefix.as_deref(),
                cursor.as_deref(),
                include_deleted,
                limit,
            )
            .await
            .map_err(|e| ApiError::new(e, request_id.0.as_str()))?;
        let next_cursor = page.next_cursor;
        let body = FilePage {
            items: page.items.iter().map(Entry::from_info).collect(),
            next_cursor,
            source: "store".into(),
            consistency: "per_file".into(),
        };
        Ok(json_response(StatusCode::OK, &body, None))
    } else if source == "index" {
        use brains_ports::ListFilter;
        let filter = ListFilter {
            prefix,
            tag,
            kind,
            include_deleted,
            order: if sort == "updated_desc" {
                brains_ports::ListOrder::UpdatedDesc
            } else {
                brains_ports::ListOrder::Path
            },
        };
        let order = filter.order;
        let (after_path, after_updated) = decode_index_cursor(cursor.as_deref())
            .map_err(|e| ApiError::new(e, request_id.0.as_str()))?;
        let page = state
            .vault
            .list_from_index(principal, filter, after_path, after_updated, limit)
            .await
            .map_err(|e| ApiError::new(e, request_id.0.as_str()))?;
        let entries: Vec<Entry> = page
            .rows
            .iter()
            .map(|row| Entry {
                path: row.path.clone(),
                revision: row.revision.clone(),
                generation: row.generation,
                state: if row.deleted {
                    HeadState::Trashed.as_str().to_owned()
                } else {
                    HeadState::Live.as_str().to_owned()
                },
                updated_at: Some(row.updated_at.clone()),
                size: Some(row.size),
                sha256: row.sha256.clone(),
                media_type: Some(row.media_type.clone()),
                kind: Some(row.kind),
                title: if row.title.is_empty() {
                    None
                } else {
                    Some(row.title.clone())
                },
            })
            .collect();
        let next_cursor = if page.exhausted {
            None
        } else {
            page.rows.last().map(|row| match order {
                brains_ports::ListOrder::UpdatedDesc => {
                    format!("{}|{}", row.updated_at, row.path)
                }
                brains_ports::ListOrder::Path => row.path.clone(),
            })
        };
        let body = FilePage {
            items: entries,
            next_cursor,
            source: "index".into(),
            consistency: "per_file".into(),
        };
        Ok(json_response(StatusCode::OK, &body, None))
    } else {
        Err(ApiError::new(
            DomainError::InvalidArgument("source must be `store` or `index`".into()),
            request_id.0.as_str(),
        ))
    }
}

fn decode_index_cursor(
    cursor: Option<&str>,
) -> brains_domain::DomainResult<(Option<String>, Option<String>)> {
    match cursor {
        None => Ok((None, None)),
        Some(raw) if raw.contains('|') => {
            let (updated, path) = raw
                .split_once('|')
                .ok_or_else(|| DomainError::InvalidArgument("malformed cursor".into()))?;
            Ok((Some(path.to_owned()), Some(updated.to_owned())))
        }
        Some(raw) => Ok((Some(raw.to_owned()), None)),
    }
}

/// GET /file — raw bytes or metadata, strong ETag, 304 on If-None-Match.
pub async fn get_file<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let path = query_path(&params, rid)?;
    let format = params.get("format").map(String::as_str).unwrap_or("raw");
    let entry = state
        .vault
        .read_entry(auth.principal(), &path)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    let etag = format!("\"{}\"", entry.revision);
    if format == "meta" {
        if not_modified(&headers, &etag) {
            return Ok(not_modified_response(&etag));
        }
        return Ok(json_response(
            StatusCode::OK,
            &Entry::from_info(&entry),
            Some(etag),
        ));
    }
    if not_modified(&headers, &etag) {
        return Ok(not_modified_response(&etag));
    }
    let content = state
        .vault
        .read_file(auth.principal(), &path)
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    let media_type = content
        .revision
        .content
        .as_ref()
        .map(|c| c.media_type.clone())
        .unwrap_or_else(|| "application/octet-stream".into());
    Ok(bytes_response(
        StatusCode::OK,
        &content.bytes,
        &media_type,
        path.file_name(),
        Some(etag),
    ))
}

/// HEAD /file — headers only; errors carry `X-Brain-Error-Code`, no body.
pub async fn head_file<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let rid_str = request_id.0.as_str();
    let mut builder = Response::builder();
    let path = match query_path(&params, rid_str) {
        Ok(path) => path,
        Err(e) => return head_error(e, None),
    };
    match state.vault.read_entry(auth.principal(), &path).await {
        Ok(entry) => {
            let etag = format!("\"{}\"", entry.revision);
            builder = builder
                .status(StatusCode::OK)
                .header(header::ETAG, etag)
                .header("X-Brain-Generation", entry.generation.to_string())
                .header("X-Brain-State", entry.state.as_str());
        }
        Err(error) => {
            let etag = error.current_revision().map(|rev| format!("\"{rev}\""));
            return head_error(ApiError::new(error, rid_str), etag);
        }
    }
    builder
        .body(axum::body::Body::empty())
        .expect("static head response")
}

fn head_error(error: ApiError, etag: Option<String>) -> Response {
    let mut response = Response::builder()
        .status(error.status())
        .header("X-Brain-Error-Code", error.error.code());
    if let Some(etag) = etag {
        response = response.header(header::ETAG, etag);
    }
    response
        .body(axum::body::Body::empty())
        .expect("static head error response")
}

fn not_modified_response(etag: &str) -> Response {
    Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header(header::ETAG, etag)
        .body(axum::body::Body::empty())
        .expect("static 304 response")
}

/// Shared raw-byte response with serving safety rules (design §15.3).
pub fn bytes_response(
    status: StatusCode,
    bytes: &[u8],
    media_type: &str,
    file_name: &str,
    etag: Option<String>,
) -> Response {
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, media_type)
        .header("X-Content-Type-Options", "nosniff");
    if let Some(disposition) = content_disposition(media_type, file_name) {
        builder = builder.header(header::CONTENT_DISPOSITION, disposition);
    }
    let mut response = builder
        .body(axum::body::Body::from(bytes.to_vec()))
        .expect("static bytes response");
    if let Some(etag) = etag
        && let Ok(value) = header::HeaderValue::from_str(&etag)
    {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}

/// PUT /file — create (201) or conditionally replace (200).
pub async fn put_file<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let path = query_path(&params, rid)?;
    if let Some(cookie) = auth.cookie_value() {
        guard_cookie_mutation(
            state.security.as_ref(),
            &headers,
            cookie,
            &state.allowed_origins,
        )
        .map_err(|e| ApiError::new(e, rid))?;
    }
    let precondition = parse_write_precondition(&headers, rid)?;
    let op_id = operation_id(&headers, rid)?;
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_owned());

    // Binary uploads must carry the digest header (design §22.3); the actual
    // bytes are always verified against it before HEAD CAS.
    let declared_sha = single_sha_header(&headers, rid)?;
    let effective_media = media_type
        .clone()
        .unwrap_or_else(|| brains_domain::media_type_for(path.as_str()).to_owned());
    if !brains_domain::is_text_media(&effective_media) && declared_sha.is_none() {
        return Err(ApiError::new(
            DomainError::InvalidArgument(
                "binary PUT requires X-Content-SHA256 (64 lowercase hex)".into(),
            ),
            rid,
        ));
    }
    // A declared digest that contradicts the actual bytes never reaches CAS
    // (design §22.3); the commit itself re-verifies against the same digest.
    if let Some(declared) = &declared_sha {
        let actual = hex::encode(Sha256::digest(&body));
        if &actual != declared {
            return Err(ApiError::new(
                DomainError::ChecksumMismatch {
                    path: path.as_str().to_owned(),
                },
                rid,
            ));
        }
    }

    let outcome = state
        .vault
        .put_file(
            auth.principal(),
            CommitInput {
                path: &path,
                content: &body,
                media_type: media_type.as_deref(),
                actor: auth.principal().actor.clone(),
                operation_id: op_id,
                precondition,
                executable: false,
                copied_from: None,
            },
        )
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    let status = if outcome.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok(json_response(
        status,
        &mutation_result(&outcome),
        Some(format!("\"{}\"", outcome.revision)),
    ))
}

fn single_sha_header(headers: &HeaderMap, rid: &str) -> ApiResult<Option<String>> {
    crate::routes::single_header(
        headers,
        header::HeaderName::from_static("x-content-sha256"),
        rid,
    )?
    .map(|raw| {
        if raw.len() == 64 && raw.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            Ok(raw)
        } else {
            Err(ApiError::new(
                DomainError::InvalidArgument("X-Content-SHA256 must be 64 lowercase hex".into()),
                rid,
            ))
        }
    })
    .transpose()
}

/// DELETE /file — soft delete via tombstone revision.
pub async fn delete_file<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let path = query_path(&params, rid)?;
    if let Some(cookie) = auth.cookie_value() {
        guard_cookie_mutation(
            state.security.as_ref(),
            &headers,
            cookie,
            &state.allowed_origins,
        )
        .map_err(|e| ApiError::new(e, rid))?;
    }
    let if_match = parse_required_if_match(&headers, rid)?;
    let op_id = operation_id(&headers, rid)?;
    // ?purge=true upgrades the tombstone delete into a permanent delete
    // (owner decision 2026-10): the entry and its reachable history vanish.
    // Same precondition shape — If-Match must name the tombstone revision.
    if params.get("purge").map(String::as_str) == Some("true") {
        let outcome = state
            .vault
            .purge_file(
                auth.principal(),
                &path,
                Precondition::IfRevision(if_match),
                op_id,
            )
            .await
            .map_err(|e| ApiError::new(e, rid))?;
        return Ok(json_response(
            StatusCode::OK,
            &PurgeResult {
                path: outcome.path.clone(),
                purged: true,
                index_state: outcome.index_state.as_str().to_owned(),
                operation_id: outcome.operation_id.clone(),
            },
            None,
        ));
    }
    let outcome = state
        .vault
        .trash_file(
            auth.principal(),
            &path,
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

/// PATCH /file — P1 exact-text edits.
pub async fn patch_file<H: Head, R: Rev, B: Blob, I: Idx>(
    (state, auth, Extension(request_id)): HandlerState<H, R, B, I>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<PatchBody>,
) -> ApiResult<Response> {
    let rid = request_id.0.as_str();
    let path = query_path(&params, rid)?;
    if let Some(cookie) = auth.cookie_value() {
        guard_cookie_mutation(
            state.security.as_ref(),
            &headers,
            cookie,
            &state.allowed_origins,
        )
        .map_err(|e| ApiError::new(e, rid))?;
    }
    let if_match = parse_required_if_match(&headers, rid)?;
    let op_id = operation_id(&headers, rid)?;
    if body.edits.len() > 20 {
        return Err(ApiError::new(
            DomainError::InvalidArgument("at most 20 edits per patch".into()),
            rid,
        ));
    }
    let edits: Vec<brains_application::patch::PatchEdit> = body
        .edits
        .into_iter()
        .map(|edit| {
            if edit.expected_matches != 1 {
                return Err(ApiError::new(
                    DomainError::InvalidArgument("P0/P1 supports expected_matches=1 only".into()),
                    rid,
                ));
            }
            Ok(brains_application::patch::PatchEdit {
                old_text: edit.old_text,
                new_text: edit.new_text,
                expected_matches: 1,
            })
        })
        .collect::<ApiResult<Vec<_>>>()?;
    let outcome = state
        .vault
        .patch_file(
            auth.principal(),
            &path,
            Precondition::IfRevision(if_match),
            auth.principal().actor.clone(),
            op_id,
            brains_application::patch::PatchInput {
                edits,
                dry_run: body.dry_run,
            },
        )
        .await
        .map_err(|e| ApiError::new(e, rid))?;
    match outcome.committed {
        None => Ok(json_response(
            StatusCode::OK,
            &PatchDryRun {
                diff: outcome.diff,
                base_revision: outcome.base_revision,
            },
            None,
        )),
        Some(committed) => Ok(json_response(
            StatusCode::OK,
            &mutation_result(&committed),
            Some(format!("\"{}\"", committed.revision)),
        )),
    }
}

pub fn mutation_result(outcome: &brains_application::MutationOutcome) -> MutationResult {
    MutationResult {
        entry: Entry {
            path: outcome.path.clone(),
            revision: outcome.revision.clone(),
            generation: outcome.generation,
            state: HeadState::Live.as_str().to_owned(),
            updated_at: Some(outcome.updated_at.clone()),
            size: Some(outcome.size),
            sha256: if outcome.sha256.is_empty() {
                None
            } else {
                Some(outcome.sha256.clone())
            },
            media_type: Some(outcome.media_type.clone()),
            kind: Some(outcome.kind),
            title: None,
        },
        index_state: outcome.index_state.as_str().to_owned(),
        operation_id: outcome.operation_id.clone(),
    }
}

fn clamp_limit(
    raw: Option<&String>,
    default: usize,
    max: usize,
) -> brains_domain::DomainResult<usize> {
    match raw {
        None => Ok(default),
        Some(raw) => {
            let value: usize = raw
                .parse()
                .map_err(|_| DomainError::InvalidArgument("limit must be an integer".into()))?;
            if value < 1 || value > max {
                return Err(DomainError::InvalidArgument(format!(
                    "limit must be between 1 and {max}"
                )));
            }
            Ok(value)
        }
    }
}
