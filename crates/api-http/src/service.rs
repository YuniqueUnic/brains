//! Implementations of the generated `contract::brains` traits: the handler
//! bodies that used to live in hand-written route modules, now feeding the
//! IDL-driven transport. The domain never names a status — failures classify
//! into the typed `raises(...)` exceptions declared in `api/brains.idl`,
//! and the generated server serializes them (status, ETag headers, the
//! `{"error": ErrorDetail}` envelope).

use brains_application::Precondition;
use brains_application::commit::CommitInput;
use brains_domain::{DomainError, HeadState, HexId, VaultPath};
use sha2::{Digest, Sha256};

use crate::auth::AuthSource;
use crate::context::current;
use crate::contract::brains as contract;
use crate::contract::brains::*;
use crate::state::{Blob, Head, Idx, Rev, SharedState};

// -- Domain failure → typed exception -------------------------------------

/// The exception categories `api/brains.idl` declares. `classify` maps every
/// domain failure onto exactly one; operations list the categories they
/// raise and `error_from_domain!` packs them into that operation's enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    BadRequest,
    Forbidden,
    NotFound,
    Gone,
    Conflict,
    PreconditionFailed,
    PreconditionRequired,
    PayloadTooLarge,
    UnprocessableEntity,
    NotImplemented,
    ServiceUnavailable,
    Internal,
}

fn classify(err: &DomainError) -> Failure {
    match err {
        DomainError::InvalidArgument(_) => Failure::BadRequest,
        DomainError::NotFound { .. } => Failure::NotFound,
        DomainError::EditMismatch { .. } | DomainError::PurgeLive { .. } => Failure::Conflict,
        DomainError::Trashed { .. } => Failure::Gone,
        DomainError::RevisionConflict { .. } => Failure::PreconditionFailed,
        DomainError::TooLarge { .. } => Failure::PayloadTooLarge,
        DomainError::InvalidFormat(_) | DomainError::ChecksumMismatch { .. } => {
            Failure::UnprocessableEntity
        }
        DomainError::PreconditionRequired => Failure::PreconditionRequired,
        DomainError::CapabilityUnsupported(_) => Failure::NotImplemented,
        DomainError::IndexUnavailable(_) | DomainError::StorageUnavailable(_) => {
            Failure::ServiceUnavailable
        }
        DomainError::Internal(_) => Failure::Internal,
        DomainError::Forbidden(_) => Failure::Forbidden,
        // Authentication failures are handled before dispatch.
        DomainError::Unauthenticated => Failure::Internal,
    }
}

/// `{"error": ErrorDetail}` payload for one domain failure.
fn detail(err: &DomainError, request_id: &str) -> contract::ErrorDetail {
    contract::ErrorDetail {
        code: err.code().to_owned(),
        message: err.to_string(),
        request_id: request_id.to_owned(),
        path: err.path().map(str::to_owned),
        current_revision: err.current_revision().map(str::to_owned),
    }
}

/// The current-HEAD ETag a 410/412 carries alongside its envelope.
fn etag_of(err: &DomainError) -> Option<String> {
    err.current_revision().map(|rev| format!("\"{rev}\""))
}

macro_rules! exception {
    (Gone, $err:expr, $rid:expr) => {
        contract::Gone {
            error: detail($err, $rid),
            etag: etag_of($err),
        }
    };
    (PreconditionFailed, $err:expr, $rid:expr) => {
        contract::PreconditionFailed {
            error: detail($err, $rid),
            etag: etag_of($err),
        }
    };
    ($variant:ident, $err:expr, $rid:expr) => {
        contract::$variant {
            error: detail($err, $rid),
        }
    };
}

/// One conversion from a domain failure to an operation's typed error enum.
/// The variant list must mirror the operation's `raises(...)` in the IDL and
/// always include `Internal`, the match fallback.
macro_rules! error_from_domain {
    ($name:ident, $enum:ident, [$($variant:ident),+ $(,)?]) => {
        fn $name(err: DomainError, rid: &str) -> $enum {
            match classify(&err) {
                $(Failure::$variant => $enum::$variant(exception!($variant, &err, rid)),)+
                // `Internal` is present in every list and absorbs the rest.
                _ => $enum::Internal(exception!(Internal, &err, rid)),
            }
        }
    };
}

error_from_domain!(
    list_files_error,
    FilesListFilesError,
    [BadRequest, Forbidden, ServiceUnavailable, Internal]
);
error_from_domain!(
    put_file_error,
    FilesPutFileError,
    [
        BadRequest,
        Forbidden,
        Gone,
        PreconditionRequired,
        PreconditionFailed,
        PayloadTooLarge,
        UnprocessableEntity,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    get_file_error,
    FilesGetFileError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        Gone,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    delete_file_error,
    FilesDeleteFileError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        Gone,
        PreconditionRequired,
        PreconditionFailed,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    purge_file_error,
    FilesPurgeFileError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        Conflict,
        PreconditionRequired,
        PreconditionFailed,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    patch_file_error,
    FilesPatchFileError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        Gone,
        Conflict,
        PreconditionRequired,
        PreconditionFailed,
        PayloadTooLarge,
        UnprocessableEntity,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    list_revisions_error,
    FilesListRevisionsError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        Gone,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    get_revision_error,
    FilesGetRevisionError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        Gone,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    restore_error,
    FilesRestoreError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        Gone,
        PreconditionRequired,
        PreconditionFailed,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    search_error,
    FilesSearchError,
    [BadRequest, Forbidden, ServiceUnavailable, Internal]
);
error_from_domain!(
    links_error,
    FilesLinksError,
    [BadRequest, Forbidden, ServiceUnavailable, Internal]
);
error_from_domain!(
    copy_error,
    FilesCopyError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        Gone,
        PreconditionFailed,
        UnprocessableEntity,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    move_error,
    FilesMoveError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        Gone,
        PreconditionFailed,
        UnprocessableEntity,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    diff_error,
    FilesDiffError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    reindex_error,
    FilesReindexError,
    [BadRequest, Forbidden, ServiceUnavailable, Internal]
);

// -- Payload conversions ---------------------------------------------------

/// Entry fields from an authoritative store row (design §11.1).
fn entry(info: &brains_domain::EntryInfo) -> contract::Entry {
    contract::Entry {
        path: info.path.clone(),
        revision: info.revision.clone(),
        generation: info.generation,
        state: info.state.as_str().to_owned(),
        updated_at: Some(info.updated_at.clone()),
        size: info.size,
        sha256: info.sha256.clone(),
        media_type: info.media_type.clone(),
        kind: info.kind.map(|k| k.as_str().to_owned()),
        title: info.title.clone(),
    }
}

fn mutation_result(outcome: &brains_application::MutationOutcome) -> contract::MutationResult {
    contract::MutationResult {
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
        kind: Some(outcome.kind.as_str().to_owned()),
        title: None,
        index_state: outcome.index_state.as_str().to_owned(),
        operation_id: outcome.operation_id.clone(),
        created: Some(outcome.created),
    }
}

fn revision_dto(rev: &brains_domain::Revision, commit_state: &str) -> contract::Revision {
    contract::Revision {
        revision: rev.revision.as_str().to_owned(),
        path: rev.path.clone(),
        generation: rev.generation,
        parent: rev.parent.as_ref().map(|p| p.as_str().to_owned()),
        action: rev.action.as_str().to_owned(),
        actor: Some(rev.actor.clone()),
        operation_id: Some(rev.operation_id.as_str().to_owned()),
        created_at: rev.created_at.clone(),
        restores_revision: rev
            .restores_revision
            .as_ref()
            .map(|r| r.as_str().to_owned()),
        copied_from: rev.copied_from.as_ref().map(|c| contract::CopiedFrom {
            path: c.path.clone(),
            revision: c.revision.as_str().to_owned(),
        }),
        commit_state: Some(commit_state.to_owned()),
        content: rev.content.as_ref().map(|c| contract::ContentRef {
            size: c.size,
            sha256: c.sha256.clone(),
            media_type: c.media_type.clone(),
            executable: c.executable,
        }),
    }
}

fn runtime(runtime: brains_domain::Runtime) -> contract::Runtime {
    match runtime {
        brains_domain::Runtime::LocalRust => contract::Runtime::LocalRust,
        brains_domain::Runtime::CloudflareRust => contract::Runtime::CloudflareRust,
        brains_domain::Runtime::CloudflareTypescript => contract::Runtime::CloudflareTypescript,
    }
}

fn capabilities(
    state: &SharedState<impl Head, impl Rev, impl Blob, impl Idx>,
) -> contract::Capabilities {
    let caps = &state.capabilities;
    contract::Capabilities {
        api_version: caps.api_version.clone(),
        store_format: caps.store_format,
        runtime: runtime(caps.runtime),
        limits: contract::CapabilityLimits {
            max_text_bytes: caps.limits.max_text_bytes as u64,
            max_attachment_bytes: caps.limits.max_attachment_bytes as u64,
        },
        features: caps.features.clone().into_iter().collect(),
        vault: caps.vault.clone(),
    }
}

// HEAD has no JSON body, so its declared exceptions carry the error code.
fn head_file_error(error: DomainError) -> contract::FilesHeadFileError {
    macro_rules! failure {
        ($variant:ident) => {
            contract::FilesHeadFileError::$variant(contract::$variant {
                code: error.code().to_owned(),
            })
        };
    }
    match classify(&error) {
        Failure::BadRequest => failure!(HeadBadRequest),
        Failure::Forbidden => failure!(HeadForbidden),
        Failure::NotFound => failure!(HeadNotFound),
        Failure::Gone => contract::FilesHeadFileError::HeadGone(contract::HeadGone {
            code: error.code().to_owned(),
            etag: etag_of(&error),
        }),
        Failure::ServiceUnavailable => failure!(HeadServiceUnavailable),
        _ => failure!(HeadInternal),
    }
}

// -- Precondition / conditional-read helpers --------------------------------

/// GET freshness check (RFC 9110 §13.1.2): weak comparison; `*` and
/// comma-separated candidate lists all match.
fn not_modified(if_none_match: Option<&str>, etag: &str) -> bool {
    let Some(raw) = if_none_match else {
        return false;
    };
    let current = etag.trim().trim_start_matches("W/").trim_matches('"');
    raw.split(',').map(str::trim).any(|candidate| {
        let candidate = candidate.trim_start_matches("W/").trim_matches('"');
        candidate == "*" || candidate == current
    })
}

/// A required If-Match (DELETE / restore / patch): absence is 428.
fn required_if_match(if_match: Option<&str>) -> Result<HexId, DomainError> {
    let raw = if_match.ok_or(DomainError::PreconditionRequired)?;
    parse_strong_etag(raw).ok_or_else(|| {
        DomainError::InvalidArgument(
            "If-Match must be one strong ETag for the base revision".into(),
        )
    })
}

/// Exactly one write precondition, strictly formed (§8.1): weak ETags,
/// wildcards for updates, both conditions present all fail.
fn write_precondition(
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> Result<Precondition, DomainError> {
    match (if_match, if_none_match) {
        (Some(_), Some(_)) => Err(DomainError::InvalidArgument(
            "provide exactly one of If-Match or If-None-Match".into(),
        )),
        (Some(if_match), None) => Ok(Precondition::IfRevision(
            parse_strong_etag(if_match).ok_or(DomainError::InvalidArgument(
                "If-Match must be one strong ETag for the base revision".into(),
            ))?,
        )),
        (None, Some("*")) => Ok(Precondition::Create),
        (None, Some(_)) => Err(DomainError::InvalidArgument(
            "If-None-Match for writes only supports `*`".into(),
        )),
        (None, None) => Err(DomainError::PreconditionRequired),
    }
}

fn parse_strong_etag(value: &str) -> Option<HexId> {
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

/// Content-Disposition policy (§13.3, §15.3): HTML/SVG and PDF download;
/// everything else may render inline.
fn content_disposition(media_type: &str, file_name: &str) -> Option<String> {
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

/// Raw-byte reads expose the file's own media type and the download policy
/// through declared response headers; the body itself is the union's
/// application/octet-stream case.
fn raw_headers(media_type: &str, file_name: &str) -> (Option<String>, Option<String>) {
    (
        Some(media_type.to_owned()),
        content_disposition(media_type, file_name),
    )
}

fn clamp_limit(raw: Option<i64>, default: usize, max: usize) -> Result<usize, DomainError> {
    match raw {
        None => Ok(default),
        Some(value) if (1..=max as i64).contains(&value) => Ok(value as usize),
        Some(_) => Err(DomainError::InvalidArgument(format!(
            "limit must be between 1 and {max}"
        ))),
    }
}

fn parse_operation_id(raw: Option<&String>) -> Result<HexId, DomainError> {
    match raw {
        None => Ok(HexId::generate()),
        Some(raw) => HexId::from_hex(raw),
    }
}

pub(crate) mod archive;

// -- Services ---------------------------------------------------------------

pub struct FilesService<H: Head, R: Rev, B: Blob, I: Idx> {
    pub state: SharedState<H, R, B, I>,
}

impl<H: Head, R: Rev, B: Blob, I: Idx> Clone for FilesService<H, R, B, I> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

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

enum TransferKind {
    Copy,
    Move,
}

/// A transfer's outcomes: in-band data (including partial failures, which
/// the IDL raises as `TransferConflict`), or a domain failure.
enum TransferOutcome {
    Done(contract::TransferResult),
    Partial(contract::TransferResult),
    Failed(DomainError),
}

#[async_trait::async_trait]
impl<H: Head, R: Rev, B: Blob, I: Idx> contract::Files for FilesService<H, R, B, I> {
    async fn capabilities(&self) -> ::xidl_rust_axum::Result<contract::Capabilities> {
        Ok(capabilities(&self.state))
    }

    async fn list_files(
        &self,
        source: Option<String>,
        prefix: Option<String>,
        limit: Option<i64>,
        cursor: Option<String>,
        tag: Option<String>,
        kind: Option<String>,
        sort: Option<String>,
        include_deleted: Option<bool>,
    ) -> Result<contract::FilePage, contract::FilesListFilesError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let query = brains_application::listing::ListQuery {
            source: source
                .as_deref()
                .unwrap_or("store")
                .parse()
                .map_err(|e| list_files_error(e, rid))?,
            prefix: prefix.filter(|v| !v.is_empty()),
            limit: clamp_limit(
                limit,
                self.state.limits.list_page_default,
                self.state.limits.list_page_max,
            )
            .map_err(|e| list_files_error(e, rid))?,
            cursor: cursor.filter(|v| !v.is_empty()),
            tag: tag.filter(|v| !v.is_empty()),
            kind: kind.as_deref().and_then(brains_domain::FileKind::parse),
            sort: if sort.as_deref() == Some("updated_desc") {
                brains_ports::ListOrder::UpdatedDesc
            } else {
                brains_ports::ListOrder::Path
            },
            include_deleted: include_deleted.unwrap_or(false),
        };
        let page = self
            .state
            .vault
            .list_files(ctx.auth.principal(), query)
            .await
            .map_err(|e| list_files_error(e, rid))?;
        Ok(contract::FilePage {
            items: page.items.iter().map(entry).collect(),
            next_cursor: page.next_cursor,
            source: page.source.to_string(),
            consistency: "per_file".into(),
        })
    }

    async fn put_file(
        &self,
        path: String,
        if_match: Option<String>,
        if_none_match: Option<String>,
        operation_id: Option<String>,
        content_sha256: Option<String>,
        executable: Option<bool>,
        content_type: Option<String>,
        body: Vec<u8>,
    ) -> Result<contract::FilesPutFileResponse, contract::FilesPutFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| put_file_error(e, rid))?;
        let precondition = write_precondition(if_match.as_deref(), if_none_match.as_deref())
            .map_err(|e| put_file_error(e, rid))?;
        let op_id =
            parse_operation_id(operation_id.as_ref()).map_err(|e| put_file_error(e, rid))?;
        let media_type = content_type.map(|value| value.trim().to_owned());
        let effective_media = media_type
            .clone()
            .unwrap_or_else(|| brains_domain::media_type_for(path.as_str()).to_owned());
        // Binary uploads must carry the digest header (§22.3); the bytes are
        // always verified against it before HEAD CAS.
        let declared_sha = content_sha256.as_deref();
        if let Some(declared) = declared_sha
            && (declared.len() != 64
                || !declared
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        {
            return Err(put_file_error(
                DomainError::InvalidArgument("X-Content-SHA256 must be 64 lowercase hex".into()),
                rid,
            ));
        }
        if !brains_domain::is_text_media(&effective_media) && declared_sha.is_none() {
            return Err(put_file_error(
                DomainError::InvalidArgument(
                    "binary PUT requires X-Content-SHA256 (64 lowercase hex)".into(),
                ),
                rid,
            ));
        }
        // A declared digest that contradicts the actual bytes never reaches
        // CAS (§22.3); the commit re-verifies against the same digest.
        if let Some(declared) = declared_sha {
            let actual = hex::encode(Sha256::digest(&body));
            if actual != declared {
                return Err(put_file_error(
                    DomainError::ChecksumMismatch {
                        path: path.as_str().to_owned(),
                    },
                    rid,
                ));
            }
        }
        let outcome = self
            .state
            .vault
            .put_file(
                ctx.auth.principal(),
                CommitInput {
                    path: &path,
                    content: &body,
                    media_type: media_type.as_deref(),
                    actor: ctx.auth.principal().actor.clone(),
                    operation_id: op_id,
                    precondition,
                    executable,
                    copied_from: None,
                },
            )
            .await
            .map_err(|e| put_file_error(e, rid))?;
        Ok(contract::FilesPutFileResponse {
            r#return: mutation_result(&outcome),
            etag: Some(format!("\"{}\"", outcome.revision)),
        })
    }

    async fn get_file(
        &self,
        path: String,
        format: Option<String>,
        if_none_match: Option<String>,
    ) -> Result<contract::FilesGetFileResponse, contract::FilesGetFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| get_file_error(e, rid))?;
        let head = self
            .state
            .vault
            .read_entry(ctx.auth.principal(), &path)
            .await
            .map_err(|e| get_file_error(e, rid))?;
        let etag = format!("\"{}\"", head.revision);
        if not_modified(if_none_match.as_deref(), &etag) {
            return Err(contract::FilesGetFileError::NotModified(
                contract::NotModified { etag: etag.clone() },
            ));
        }
        if format.as_deref() == Some("meta") {
            return Ok(contract::FilesGetFileResponse {
                r#return: contract::FilePayload::Json(entry(&head)),
                etag: Some(etag),
                media_type: None,
                content_disposition: None,
            });
        }
        let content = self
            .state
            .vault
            .read_file(ctx.auth.principal(), &path)
            .await
            .map_err(|e| get_file_error(e, rid))?;
        let media = content
            .revision
            .content
            .as_ref()
            .map(|c| c.media_type.clone())
            .unwrap_or_else(|| "application/octet-stream".into());
        let (media_type, content_disposition) = raw_headers(&media, path.file_name());
        Ok(contract::FilesGetFileResponse {
            r#return: contract::FilePayload::OctetStream(content.bytes),
            etag: Some(etag),
            media_type,
            content_disposition,
        })
    }

    async fn head_file(
        &self,
        path: String,
    ) -> Result<contract::FilesHeadFileResponse, contract::FilesHeadFileError> {
        let ctx = current();
        let path = VaultPath::new(&path).map_err(head_file_error)?;
        let head = self
            .state
            .vault
            .read_entry(ctx.auth.principal(), &path)
            .await
            .map_err(head_file_error)?;
        Ok(contract::FilesHeadFileResponse {
            etag: format!("\"{}\"", head.revision),
            generation: head.generation,
            state: head.state.as_str().to_owned(),
        })
    }

    async fn delete_file(
        &self,
        path: String,
        if_match: Option<String>,
        operation_id: Option<String>,
    ) -> Result<contract::FilesDeleteFileResponse, contract::FilesDeleteFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| delete_file_error(e, rid))?;
        let if_match =
            required_if_match(if_match.as_deref()).map_err(|e| delete_file_error(e, rid))?;
        let op_id =
            parse_operation_id(operation_id.as_ref()).map_err(|e| delete_file_error(e, rid))?;
        let outcome = self
            .state
            .vault
            .trash_file(
                ctx.auth.principal(),
                &path,
                Precondition::IfRevision(if_match),
                ctx.auth.principal().actor.clone(),
                op_id,
            )
            .await
            .map_err(|e| delete_file_error(e, rid))?;
        Ok(contract::FilesDeleteFileResponse {
            r#return: mutation_result(&outcome),
            etag: Some(format!("\"{}\"", outcome.revision)),
        })
    }

    async fn purge_file(
        &self,
        path: String,
        if_match: Option<String>,
        operation_id: Option<String>,
    ) -> Result<contract::PurgeResult, contract::FilesPurgeFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| purge_file_error(e, rid))?;
        let revision =
            required_if_match(if_match.as_deref()).map_err(|e| purge_file_error(e, rid))?;
        let operation_id =
            parse_operation_id(operation_id.as_ref()).map_err(|e| purge_file_error(e, rid))?;
        let outcome = self
            .state
            .vault
            .purge_file(
                ctx.auth.principal(),
                &path,
                Precondition::IfRevision(revision),
                operation_id,
            )
            .await
            .map_err(|e| purge_file_error(e, rid))?;
        Ok(contract::PurgeResult {
            path: outcome.path,
            purged: true,
            index_state: outcome.index_state.as_str().to_owned(),
            operation_id: outcome.operation_id,
        })
    }

    async fn patch_file(
        &self,
        path: String,
        if_match: Option<String>,
        operation_id: Option<String>,
        body: contract::PatchBody,
    ) -> Result<contract::FilesPatchFileResponse, contract::FilesPatchFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| patch_file_error(e, rid))?;
        let if_match =
            required_if_match(if_match.as_deref()).map_err(|e| patch_file_error(e, rid))?;
        let op_id =
            parse_operation_id(operation_id.as_ref()).map_err(|e| patch_file_error(e, rid))?;
        if body.edits.len() > 20 {
            return Err(patch_file_error(
                DomainError::InvalidArgument("at most 20 edits per patch".into()),
                rid,
            ));
        }
        let mut edits = Vec::with_capacity(body.edits.len());
        for edit in body.edits {
            if edit.expected_matches != 1 {
                return Err(patch_file_error(
                    DomainError::InvalidArgument("P0/P1 supports expected_matches=1 only".into()),
                    rid,
                ));
            }
            edits.push(brains_application::patch::PatchEdit {
                old_text: edit.old_text,
                new_text: edit.new_text,
                expected_matches: 1,
            });
        }
        let outcome = self
            .state
            .vault
            .patch_file(
                ctx.auth.principal(),
                &path,
                Precondition::IfRevision(if_match),
                ctx.auth.principal().actor.clone(),
                op_id,
                brains_application::patch::PatchInput {
                    edits,
                    dry_run: body.dry_run.unwrap_or(false),
                },
            )
            .await
            .map_err(|e| patch_file_error(e, rid))?;
        let committed = outcome.committed.as_ref().map(mutation_result);
        let etag = committed
            .as_ref()
            .map(|result| format!("\"{}\"", result.revision));
        Ok(contract::FilesPatchFileResponse {
            r#return: contract::PatchResult {
                diff: outcome.diff,
                base_revision: outcome.base_revision,
                committed,
            },
            etag,
        })
    }

    async fn list_revisions(
        &self,
        path: String,
        limit: Option<i64>,
        cursor: Option<String>,
    ) -> Result<contract::HistoryPage, contract::FilesListRevisionsError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| list_revisions_error(e, rid))?;
        let limit = clamp_limit(
            limit,
            self.state.limits.history_page_max,
            self.state.limits.history_page_max,
        )
        .map_err(|e| list_revisions_error(e, rid))?;
        let cursor = cursor.filter(|c| !c.is_empty());
        let page = self
            .state
            .vault
            .history_page(ctx.auth.principal(), &path, cursor.as_deref(), limit)
            .await
            .map_err(|e| list_revisions_error(e, rid))?;
        Ok(contract::HistoryPage {
            items: page
                .items
                .iter()
                .map(|rev| revision_dto(rev, "reachable"))
                .collect(),
            anchor_revision: page.anchor_revision,
            next_cursor: page.next_cursor,
        })
    }

    async fn get_revision(
        &self,
        path: String,
        revision: String,
        format: Option<String>,
    ) -> Result<contract::FilesGetRevisionResponse, contract::FilesGetRevisionError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| get_revision_error(e, rid))?;
        let revision_id = HexId::from_hex(&revision).map_err(|e| get_revision_error(e, rid))?;
        let record = self
            .state
            .vault
            .read_revision(ctx.auth.principal(), &path, &revision_id)
            .await
            .map_err(|e| get_revision_error(e, rid))?;
        let etag = format!("\"{}\"", record.revision.as_str());
        if format.as_deref() == Some("meta") {
            return Ok(contract::FilesGetRevisionResponse {
                r#return: contract::RevisionPayload::Json(revision_dto(&record, "unknown")),
                etag: Some(etag),
                media_type: None,
                content_disposition: None,
            });
        }
        let bytes = self
            .state
            .vault
            .revision_bytes(&record)
            .await
            .map_err(|e| get_revision_error(e, rid))?;
        let media = record
            .content
            .as_ref()
            .map(|c| c.media_type.clone())
            .unwrap_or_else(|| "application/octet-stream".into());
        let (media_type, content_disposition) = raw_headers(&media, path.file_name());
        Ok(contract::FilesGetRevisionResponse {
            r#return: contract::RevisionPayload::OctetStream(bytes),
            etag: Some(etag),
            media_type,
            content_disposition,
        })
    }

    async fn restore(
        &self,
        if_match: Option<String>,
        operation_id: Option<String>,
        body: contract::RestoreBody,
    ) -> Result<contract::FilesRestoreResponse, contract::FilesRestoreError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&body.path).map_err(|e| restore_error(e, rid))?;
        let if_match = required_if_match(if_match.as_deref()).map_err(|e| restore_error(e, rid))?;
        let op_id = parse_operation_id(operation_id.as_ref()).map_err(|e| restore_error(e, rid))?;
        let source = HexId::from_hex(&body.revision).map_err(|e| restore_error(e, rid))?;
        let outcome = self
            .state
            .vault
            .restore_file(
                ctx.auth.principal(),
                &path,
                &source,
                Precondition::IfRevision(if_match),
                ctx.auth.principal().actor.clone(),
                op_id,
            )
            .await
            .map_err(|e| restore_error(e, rid))?;
        Ok(contract::FilesRestoreResponse {
            r#return: mutation_result(&outcome),
            etag: Some(format!("\"{}\"", outcome.revision)),
        })
    }

    async fn search(
        &self,
        q: String,
        mode: Option<String>,
        prefix: Option<String>,
        tag: Option<String>,
        scan_limit: Option<i64>,
        cursor: Option<String>,
    ) -> Result<contract::SearchPage, contract::FilesSearchError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        if q.is_empty() {
            return Err(search_error(
                DomainError::InvalidArgument("missing `q`".into()),
                rid,
            ));
        }
        if q.len() > 512 {
            return Err(search_error(
                DomainError::InvalidArgument("q must be at most 512 bytes".into()),
                rid,
            ));
        }
        let mode = match mode.as_deref().unwrap_or("literal") {
            "literal" => brains_application::search::SearchMode::Literal,
            "fts" => brains_application::search::SearchMode::Fts,
            other => {
                return Err(search_error(
                    DomainError::InvalidArgument(format!("unknown search mode `{other}`")),
                    rid,
                ));
            }
        };
        let scan_limit = match scan_limit {
            None => self.state.limits.scan_window_max,
            Some(raw) => {
                let max = self.state.limits.scan_window_max;
                if raw < 1 || raw as usize > max {
                    return Err(search_error(
                        DomainError::InvalidArgument(format!(
                            "scan_limit must be between 1 and {max}"
                        )),
                        rid,
                    ));
                }
                raw as usize
            }
        };
        let query = brains_application::search::SearchQuery {
            q: q.clone(),
            mode,
            prefix: prefix.filter(|p| !p.is_empty()),
            tag: tag.filter(|t| !t.is_empty()),
            cursor: cursor.filter(|c| !c.is_empty()),
            scan_limit,
        };
        let result = self
            .state
            .vault
            .search(ctx.auth.principal(), query)
            .await
            .map_err(|e| search_error(e, rid))?;
        Ok(contract::SearchPage {
            items: result
                .items
                .into_iter()
                .map(|hit| contract::SearchHit {
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
        })
    }

    async fn links(
        &self,
        path: String,
        direction: Option<String>,
        cursor: Option<String>,
    ) -> Result<contract::LinkPage, contract::FilesLinksError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| links_error(e, rid))?;
        let backlinks = match direction.as_deref().unwrap_or("outbound") {
            "outbound" => false,
            "backlinks" => true,
            other => {
                return Err(links_error(
                    DomainError::InvalidArgument(format!("unknown direction `{other}`")),
                    rid,
                ));
            }
        };
        let cursor = cursor.filter(|c| !c.is_empty());
        let page = self
            .state
            .vault
            .links(
                ctx.auth.principal(),
                &path,
                backlinks,
                cursor.as_deref(),
                50,
            )
            .await
            .map_err(|e| links_error(e, rid))?;
        Ok(contract::LinkPage {
            items: page
                .rows
                .into_iter()
                .map(|row| contract::LinkItem {
                    source: row.source,
                    target: row.target,
                    kind: row.kind,
                    source_revision: Some(row.source_revision),
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    async fn copy(
        &self,
        body: contract::TransferBody,
    ) -> Result<contract::TransferResult, contract::FilesCopyError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        match self.transfer(body, TransferKind::Copy).await {
            TransferOutcome::Done(result) => Ok(result),
            TransferOutcome::Partial(result) => Err(contract::FilesCopyError::TransferConflict(
                contract::TransferConflict {
                    copied: result.copied,
                    source_trashed: result.source_trashed,
                    partial: result.partial,
                    operation_id: result.operation_id,
                    target_revision: result.target_revision,
                    source_revision: result.source_revision,
                    error: result.error,
                },
            )),
            TransferOutcome::Failed(e) => Err(copy_error(e, rid)),
        }
    }

    async fn r#move(
        &self,
        body: contract::TransferBody,
    ) -> Result<contract::TransferResult, contract::FilesMoveError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        match self.transfer(body, TransferKind::Move).await {
            TransferOutcome::Done(result) => Ok(result),
            TransferOutcome::Partial(result) => Err(contract::FilesMoveError::TransferConflict(
                contract::TransferConflict {
                    copied: result.copied,
                    source_trashed: result.source_trashed,
                    partial: result.partial,
                    operation_id: result.operation_id,
                    target_revision: result.target_revision,
                    source_revision: result.source_revision,
                    error: result.error,
                },
            )),
            TransferOutcome::Failed(e) => Err(move_error(e, rid)),
        }
    }

    async fn diff(
        &self,
        path: String,
        from: String,
        to: String,
    ) -> Result<contract::DiffResult, contract::FilesDiffError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| diff_error(e, rid))?;
        let from = HexId::from_hex(&from).map_err(|e| diff_error(e, rid))?;
        let to = HexId::from_hex(&to).map_err(|e| diff_error(e, rid))?;
        let diff = self
            .state
            .vault
            .diff_revisions(ctx.auth.principal(), &path, &from, &to)
            .await
            .map_err(|e| diff_error(e, rid))?;
        Ok(contract::DiffResult { diff })
    }

    async fn reindex(
        &self,
        body: contract::ReindexRequest,
    ) -> Result<contract::ReindexPage, contract::FilesReindexError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let limit = match body.limit {
            None => self.state.limits.reindex_page_max,
            Some(raw) => {
                let max = self.state.limits.reindex_page_max as i64;
                if !(1..=max).contains(&raw) {
                    return Err(reindex_error(
                        DomainError::InvalidArgument(format!("limit must be between 1 and {max}")),
                        rid,
                    ));
                }
                raw as usize
            }
        };
        let page = self
            .state
            .vault
            .reindex_page(ctx.auth.principal(), body.cursor.as_deref(), limit)
            .await
            .map_err(|e| reindex_error(e, rid))?;
        Ok(contract::ReindexPage {
            processed: page.processed as i64,
            failed: page
                .failed
                .into_iter()
                .map(|f| contract::ReindexFailure {
                    path: f.path,
                    code: f.code,
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    async fn validate(&self) -> Result<(), contract::FilesValidateError> {
        let ctx = current();
        let err = DomainError::CapabilityUnsupported("server_side_validate");
        Err(contract::FilesValidateError::NotImplemented(exception!(
            NotImplemented,
            &err,
            ctx.request_id.as_str()
        )))
    }
}

impl<H: Head, R: Rev, B: Blob, I: Idx> FilesService<H, R, B, I> {
    /// Copy and move share one body: validate the transfer, run it, and keep
    /// partial outcomes as data — the IDL raises them as `TransferConflict`.
    async fn transfer(&self, body: contract::TransferBody, kind: TransferKind) -> TransferOutcome {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let invalid = |message: &str| {
            TransferOutcome::Failed(DomainError::InvalidArgument(message.to_owned()))
        };
        // A header/body operation_id mismatch is a programming error (§22.5).
        if let Some(header_op) = ctx
            .headers
            .get("x-operation-id")
            .and_then(|v| v.to_str().ok())
            && header_op != body.operation_id
        {
            return invalid("X-Operation-ID does not match the body operation_id");
        }
        let Ok(source) = VaultPath::new(&body.source) else {
            return invalid("invalid `source` path");
        };
        let Ok(target) = VaultPath::new(&body.target) else {
            return invalid("invalid `target` path");
        };
        let (Ok(expected_source_revision), Ok(operation_id)) = (
            HexId::from_hex(&body.expected_source_revision),
            HexId::from_hex(&body.operation_id),
        ) else {
            return invalid("operation_id and expected_source_revision must be hex ids");
        };
        let input = brains_application::transfer::TransferInput {
            source,
            target,
            expected_source_revision,
            actor: ctx.auth.principal().actor.clone(),
            operation_id,
        };
        let result = match kind {
            TransferKind::Copy => {
                self.state
                    .vault
                    .copy_file(ctx.auth.principal(), input)
                    .await
            }
            TransferKind::Move => {
                self.state
                    .vault
                    .move_file(ctx.auth.principal(), input)
                    .await
            }
        };
        let result = match result {
            Ok(result) => result,
            Err(e) => return TransferOutcome::Failed(e),
        };
        let mapped = contract::TransferResult {
            copied: result.copied,
            source_trashed: result.source_trashed,
            partial: result.partial,
            operation_id: result.operation_id,
            target_revision: result.target_revision,
            source_revision: result.source_revision,
            error: result.error.map(|e| detail(&e, rid)),
        };
        if mapped.partial {
            TransferOutcome::Partial(mapped)
        } else {
            TransferOutcome::Done(mapped)
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
