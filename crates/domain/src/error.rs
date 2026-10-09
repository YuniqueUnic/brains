//! Domain error taxonomy. The HTTP layer maps [`ErrorCode`] to statuses;
//! domain and application code never name an HTTP status.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// 400 INVALID_ARGUMENT
    InvalidArgument(String),
    /// 422 INVALID_FORMAT — text that must parse (UTF-8, profile) does not.
    InvalidFormat(String),
    /// 422 CHECKSUM_MISMATCH — declared and actual content digests differ.
    ChecksumMismatch { path: String },
    /// 401 UNAUTHENTICATED
    Unauthenticated,
    /// 403 FORBIDDEN
    Forbidden(String),
    /// 404 NOT_FOUND
    NotFound { path: String },
    /// 410 TRASHED — the path exists as a tombstone.
    Trashed { path: String, revision: String },
    /// 409 PURGE_REQUIRES_TRASHED — permanent delete of a live path.
    PurgeLive { path: String },
    /// 409 EDIT_MISMATCH — a patch edit did not match exactly once.
    EditMismatch { path: String, detail: String },
    /// 412 REVISION_CONFLICT — the conditional version moved or was expected absent.
    RevisionConflict {
        path: String,
        current_revision: Option<String>,
    },
    /// 413 TOO_LARGE
    TooLarge { limit: usize, actual: usize },
    /// 428 PRECONDITION_REQUIRED — a mutation without a safety precondition.
    PreconditionRequired,
    /// 501 CAPABILITY_UNSUPPORTED
    CapabilityUnsupported(&'static str),
    /// 503 INDEX_UNAVAILABLE — search/links must fail loudly, never fake empty.
    IndexUnavailable(String),
    /// 503 STORAGE_UNAVAILABLE
    StorageUnavailable(String),
    /// 500 INTERNAL — bugs and unreachable states; never used for expected failures.
    Internal(String),
}

impl DomainError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidArgument(_) => "INVALID_ARGUMENT",
            Self::InvalidFormat(_) => "INVALID_FORMAT",
            Self::ChecksumMismatch { .. } => "CHECKSUM_MISMATCH",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound { .. } => "NOT_FOUND",
            Self::Trashed { .. } => "TRASHED",
            Self::PurgeLive { .. } => "PURGE_REQUIRES_TRASHED",
            Self::EditMismatch { .. } => "EDIT_MISMATCH",
            Self::RevisionConflict { .. } => "REVISION_CONFLICT",
            Self::TooLarge { .. } => "TOO_LARGE",
            Self::PreconditionRequired => "PRECONDITION_REQUIRED",
            Self::CapabilityUnsupported(_) => "CAPABILITY_UNSUPPORTED",
            Self::IndexUnavailable(_) => "INDEX_UNAVAILABLE",
            Self::StorageUnavailable(_) => "STORAGE_UNAVAILABLE",
            Self::Internal(_) => "INTERNAL",
        }
    }

    /// The logical path to attach to the API error body, if applicable.
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::ChecksumMismatch { path }
            | Self::NotFound { path }
            | Self::Trashed { path, .. }
            | Self::PurgeLive { path }
            | Self::EditMismatch { path, .. }
            | Self::RevisionConflict { path, .. } => Some(path),
            _ => None,
        }
    }

    /// The current HEAD revision: 412 conflict bodies and 410 tombstone
    /// ETags both rely on it (design §11.2: 410 carries the current ETag so
    /// the client can restore against it).
    pub fn current_revision(&self) -> Option<&str> {
        match self {
            Self::RevisionConflict {
                current_revision: Some(rev),
                ..
            } => Some(rev),
            Self::Trashed { revision, .. } => Some(revision),
            _ => None,
        }
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument(m) => write!(f, "invalid argument: {m}"),
            Self::InvalidFormat(m) => write!(f, "invalid format: {m}"),
            Self::ChecksumMismatch { path } => write!(f, "checksum mismatch: {path}"),
            Self::Unauthenticated => write!(f, "authentication required"),
            Self::Forbidden(m) => write!(f, "forbidden: {m}"),
            Self::NotFound { path } => write!(f, "not found: {path}"),
            Self::Trashed { path, revision } => {
                write!(f, "trashed: {path} (revision {revision})")
            }
            Self::PurgeLive { path } => {
                write!(f, "purge requires a trashed path: {path} is live")
            }
            Self::EditMismatch { detail, .. } => write!(f, "edit mismatch: {detail}"),
            Self::RevisionConflict {
                path,
                current_revision,
            } => {
                write!(f, "revision conflict on {path}")?;
                if let Some(rev) = current_revision {
                    write!(f, ", current is {rev}")?;
                }
                Ok(())
            }
            Self::TooLarge { limit, actual } => {
                write!(f, "body too large: {actual} bytes exceeds limit {limit}")
            }
            Self::PreconditionRequired => {
                write!(f, "a write precondition is required")
            }
            Self::CapabilityUnsupported(feature) => {
                write!(f, "capability unsupported on this instance: {feature}")
            }
            Self::IndexUnavailable(m) => write!(f, "index unavailable: {m}"),
            Self::StorageUnavailable(m) => write!(f, "storage unavailable: {m}"),
            Self::Internal(m) => write!(f, "internal error: {m}"),
        }
    }
}

impl std::error::Error for DomainError {}

pub type DomainResult<T> = Result<T, DomainError>;
