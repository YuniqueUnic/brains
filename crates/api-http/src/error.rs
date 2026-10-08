//! Domain error → HTTP mapping (design §11.3). The domain never names a
//! status; this module owns the whole table.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use brainstore_api_types::{ErrorBody, ErrorDetail};
use brainstore_domain::DomainError;

pub fn status_for(error: &DomainError) -> StatusCode {
    match error {
        DomainError::InvalidArgument(_) => StatusCode::BAD_REQUEST,
        DomainError::Unauthenticated => StatusCode::UNAUTHORIZED,
        DomainError::Forbidden(_) => StatusCode::FORBIDDEN,
        DomainError::NotFound { .. } => StatusCode::NOT_FOUND,
        DomainError::EditMismatch { .. } => StatusCode::CONFLICT,
        DomainError::Trashed { .. } => StatusCode::GONE,
        DomainError::RevisionConflict { .. } => StatusCode::PRECONDITION_FAILED,
        DomainError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        DomainError::InvalidFormat(_) | DomainError::ChecksumMismatch { .. } => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        DomainError::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
        DomainError::CapabilityUnsupported(_) => StatusCode::NOT_IMPLEMENTED,
        DomainError::IndexUnavailable(_) | DomainError::StorageUnavailable(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        DomainError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub struct ApiError {
    pub error: DomainError,
    pub request_id: String,
}

impl ApiError {
    pub fn new(error: DomainError, request_id: impl Into<String>) -> Self {
        Self {
            error,
            request_id: request_id.into(),
        }
    }

    pub fn status(&self) -> StatusCode {
        status_for(&self.error)
    }

    pub fn body(&self) -> ErrorBody {
        ErrorBody::new(ErrorDetail {
            code: self.error.code().to_owned(),
            message: self.error.to_string(),
            request_id: self.request_id.clone(),
            path: self.error.path().map(str::to_owned),
            current_revision: self.error.current_revision().map(str::to_owned),
            details: None,
        })
    }

    /// The ETag of the current HEAD revision, for 410/412 responses.
    pub fn current_etag(&self) -> Option<String> {
        self.error
            .current_revision()
            .map(|rev| format!("\"{rev}\""))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.status(), Json(self.body())).into_response();
        if let Some(etag) = self.current_etag()
            && let Ok(value) = axum::http::HeaderValue::from_str(&etag)
        {
            response
                .headers_mut()
                .insert(axum::http::header::ETAG, value);
        }
        response
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
