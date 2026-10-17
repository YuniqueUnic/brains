//! Domain failure -> typed IDL exception. `classify` maps every domain
//! failure onto exactly one declared category; each operation mirrors its
//! `raises(...)` list in one `error_from_domain!` table and always includes
//! `Internal` as the match fallback. Macro bodies use absolute paths, so a
//! call site only needs the macros themselves.

use brains_domain::DomainError;

use crate::contract::brains as contract;
use crate::contract::brains::*;

/// The exception categories `api/brains.idl` declares. `classify` maps every
/// domain failure onto exactly one; operations list the categories they
/// raise and `error_from_domain!` packs them into that operation's enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
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

pub(crate) fn classify(err: &DomainError) -> Failure {
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
pub(crate) fn detail(err: &DomainError, request_id: &str) -> contract::ErrorDetail {
    contract::ErrorDetail {
        code: err.code().to_owned(),
        message: err.to_string(),
        request_id: request_id.to_owned(),
        path: err.path().map(str::to_owned),
        current_revision: err.current_revision().map(str::to_owned),
    }
}

/// The current-HEAD ETag a 410/412 carries alongside its envelope.
pub(crate) fn etag_of(err: &DomainError) -> Option<String> {
    err.current_revision().map(|rev| format!("\"{rev}\""))
}

macro_rules! exception {
    (Gone, $err:expr, $rid:expr) => {
        crate::contract::brains::Gone {
            error: crate::service::error::detail($err, $rid),
            etag: crate::service::error::etag_of($err),
        }
    };
    (PreconditionFailed, $err:expr, $rid:expr) => {
        crate::contract::brains::PreconditionFailed {
            error: crate::service::error::detail($err, $rid),
            etag: crate::service::error::etag_of($err),
        }
    };
    ($variant:ident, $err:expr, $rid:expr) => {
        crate::contract::brains::$variant {
            error: crate::service::error::detail($err, $rid),
        }
    };
}

/// One conversion from a domain failure to an operation's typed error enum.
/// The variant list must mirror the operation's `raises(...)` in the IDL and
/// always include `Internal`, the match fallback.
macro_rules! error_from_domain {
    ($name:ident, $enum:ident, [$($variant:ident),+ $(,)?]) => {
        pub(crate) fn $name(err: DomainError, rid: &str) -> $enum {
            match crate::service::error::classify(&err) {
                $(crate::service::error::Failure::$variant => $enum::$variant(exception!($variant, &err, rid)),)+
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
    [
        BadRequest,
        Forbidden,
        NotImplemented,
        ServiceUnavailable,
        Internal
    ]
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

// HEAD has no JSON body, so its declared exceptions carry the error code.
pub(crate) fn head_file_error(error: DomainError) -> contract::FilesHeadFileError {
    macro_rules! failure {
        ($variant:ident) => {
            contract::FilesHeadFileError::$variant(crate::contract::brains::$variant {
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

pub(crate) use error_from_domain;
pub(crate) use exception;
