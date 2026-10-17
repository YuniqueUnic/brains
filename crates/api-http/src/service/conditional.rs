//! Precondition parsing (design §12.1), conditional reads (RFC 9110 weak
//! comparison), and per-request header/limit plumbing shared by the Files
//! operations.

use brains_application::Precondition;
use brains_domain::{DomainError, HexId, VaultPath};
use sha2::{Digest, Sha256};

// -- Precondition / conditional-read helpers --------------------------------

/// GET freshness check (RFC 9110 §13.1.2): weak comparison; `*` and
/// comma-separated candidate lists all match.
pub(crate) fn not_modified(if_none_match: Option<&str>, etag: &str) -> bool {
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
pub(crate) fn required_if_match(if_match: Option<&str>) -> Result<HexId, DomainError> {
    let raw = if_match.ok_or(DomainError::PreconditionRequired)?;
    parse_strong_etag(raw).ok_or_else(|| {
        DomainError::InvalidArgument(
            "If-Match must be one strong ETag for the base revision".into(),
        )
    })
}

/// Exactly one write precondition, strictly formed (§8.1): weak ETags,
/// wildcards for updates, both conditions present all fail.
pub(crate) fn write_precondition(
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

pub(crate) fn parse_strong_etag(value: &str) -> Option<HexId> {
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
pub(crate) fn content_disposition(media_type: &str, file_name: &str) -> Option<String> {
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
pub(crate) fn raw_headers(media_type: &str, file_name: &str) -> (Option<String>, Option<String>) {
    (
        Some(media_type.to_owned()),
        content_disposition(media_type, file_name),
    )
}

pub(crate) fn clamp_limit(
    raw: Option<i64>,
    default: usize,
    max: usize,
) -> Result<usize, DomainError> {
    match raw {
        None => Ok(default),
        Some(value) if (1..=max as i64).contains(&value) => Ok(value as usize),
        Some(_) => Err(DomainError::InvalidArgument(format!(
            "limit must be between 1 and {max}"
        ))),
    }
}

pub(crate) fn parse_operation_id(raw: Option<&String>) -> Result<HexId, DomainError> {
    match raw {
        None => Ok(HexId::generate()),
        Some(raw) => HexId::from_hex(raw),
    }
}

/// Upload digest validation (design §22.3): a declared digest must be 64
/// lowercase hex, binary uploads must declare one, and a declared digest
/// that contradicts the bytes never reaches HEAD CAS.
pub(crate) fn validate_declared_sha(
    declared: Option<&str>,
    is_text_media: bool,
    body: &[u8],
    path: &VaultPath,
) -> Result<(), DomainError> {
    if let Some(declared) = declared
        && (declared.len() != 64
            || !declared
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
    {
        return Err(DomainError::InvalidArgument(
            "X-Content-SHA256 must be 64 lowercase hex".into(),
        ));
    }
    if !is_text_media && declared.is_none() {
        return Err(DomainError::InvalidArgument(
            "binary PUT requires X-Content-SHA256 (64 lowercase hex)".into(),
        ));
    }
    if let Some(declared) = declared {
        let actual = hex::encode(Sha256::digest(body));
        if actual != declared {
            return Err(DomainError::ChecksumMismatch {
                path: path.as_str().to_owned(),
            });
        }
    }
    Ok(())
}
