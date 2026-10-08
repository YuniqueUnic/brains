//! RFC 3339 UTC timestamps, stored as strings end to end so persisted JSON
//! and API payloads stay byte-stable.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::{DomainError, DomainResult};

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("current time is representable")
}

pub fn validate_rfc3339(raw: &str) -> DomainResult<()> {
    OffsetDateTime::parse(raw, &Rfc3339)
        .map(|_| ())
        .map_err(|e| DomainError::InvalidFormat(format!("timestamp `{raw}` is not RFC 3339: {e}")))
}
