//! PathPolicy (design §6.2): API `path` is a vault-relative logical path,
//! never a URL or OS path.

use std::fmt;

use unicode_normalization::UnicodeNormalization;

use crate::error::{DomainError, DomainResult};
use crate::limits::Limits;

/// A validated vault-relative path in NFC form, `/`-separated.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultPath(String);

impl VaultPath {
    /// Validate against the default instance limits.
    pub fn new(raw: &str) -> DomainResult<Self> {
        Self::with_limits(raw, &Limits::default())
    }

    /// Validate against explicit instance limits.
    pub fn with_limits(raw: &str, limits: &Limits) -> DomainResult<Self> {
        validate(raw, limits)?;
        Ok(Self(raw.to_owned()))
    }

    /// Wrap a path that has already been validated (e.g. loaded from durable
    /// HEAD records, which were validated at write time).
    pub fn trusted(raw: String) -> Self {
        Self(raw)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The last `/`-segment: the file name used inside immutable blob keys.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// The directory part including the trailing slash, if any.
    pub fn parent(&self) -> Option<&str> {
        let idx = self.0.rfind('/')?;
        Some(&self.0[..=idx])
    }

    /// Segment-boundary prefix test: `projects/foo` covers `projects/foo` and
    /// everything under `projects/foo/`, but never `projects/foobar`.
    pub fn has_prefix(&self, prefix: &str) -> bool {
        prefix_has(prefix, &self.0)
    }
}

impl fmt::Display for VaultPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for VaultPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A directory prefix accepts an optional trailing slash; empty or `/` is
/// the whole vault. Containment always compares complete path segments.
pub fn normalize_prefix(prefix: Option<&str>) -> Option<&str> {
    prefix
        .map(|p| p.trim_end_matches('/'))
        .filter(|p| !p.is_empty())
}

pub fn prefix_has(prefix: &str, path: &str) -> bool {
    let Some(prefix) = normalize_prefix(Some(prefix)) else {
        return true;
    };
    if path == prefix {
        return true;
    }
    path.len() > prefix.len()
        && path.starts_with(prefix)
        && path.as_bytes().get(prefix.len()) == Some(&b'/')
}

/// Apply every structural rule of the PathPolicy. Rejects instead of
/// silently normalizing: a path that is not NFC is a client bug, not input
/// to repair.
pub fn validate(raw: &str, limits: &Limits) -> DomainResult<()> {
    if raw.is_empty() {
        return Err(invalid("path must not be empty"));
    }
    if raw.starts_with('/') {
        return Err(invalid("path must be vault-relative"));
    }
    if raw.contains('\\') {
        return Err(invalid("backslash is not a path separator"));
    }
    if raw.len() > limits.path_max_bytes {
        return Err(invalid(format!(
            "path exceeds {} bytes",
            limits.path_max_bytes
        )));
    }
    if raw.nfc().ne(raw.chars()) {
        return Err(invalid("path must be Unicode NFC"));
    }
    if raw.chars().any(is_control) {
        return Err(invalid("path must not contain control characters"));
    }
    for segment in raw.split('/') {
        if segment.is_empty() {
            return Err(invalid("empty path segment"));
        }
        if segment == "." || segment == ".." {
            return Err(invalid("`.` and `..` segments are not allowed"));
        }
        if segment.len() > limits.segment_max_bytes {
            return Err(invalid(format!(
                "path segment exceeds {} bytes",
                limits.segment_max_bytes
            )));
        }
        if segment.eq_ignore_ascii_case(".brain") || segment.eq_ignore_ascii_case(".git") {
            return Err(invalid("reserved namespace segment"));
        }
    }
    Ok(())
}

fn is_control(c: char) -> bool {
    matches!(c as u32, 0..=0x1F | 0x7F..=0x9F)
}

fn invalid(msg: impl Into<String>) -> DomainError {
    DomainError::InvalidArgument(format!("invalid path: {}", msg.into()))
}

/// Windows reserved device names (case-insensitive stem match).
const WINDOWS_RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "conin$", "conout$", "com1", "com2", "com3", "com4", "com5",
    "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7",
];

/// Portable-name checks applied at export time (design §6.2): a vault may
/// legally contain such paths, but exporting them to a real filesystem must
/// fail loudly instead of dropping a file.
pub fn check_portable_segment(segment: &str) -> Result<(), String> {
    if segment.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Err(format!("nonportable character in `{segment}`"));
    }
    if segment.ends_with('.') || segment.ends_with(' ') {
        return Err(format!("trailing dot/space in `{segment}`"));
    }
    let stem = segment.split('.').next().unwrap_or(segment);
    if WINDOWS_RESERVED
        .iter()
        .copied()
        .chain(["lpt8", "lpt9"])
        .any(|r| r.eq_ignore_ascii_case(stem))
    {
        return Err(format!("reserved device name `{stem}`"));
    }
    Ok(())
}
