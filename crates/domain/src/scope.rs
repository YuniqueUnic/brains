//! Authorization scopes (design §15.2): level × vault path prefix, compared
//! at segment boundaries. A principal's actor string is server-derived from
//! authentication, never from request content.

use crate::error::{DomainError, DomainResult};
use crate::path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AccessLevel {
    Read,
    Write,
    Admin,
}

impl AccessLevel {
    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "admin" => Ok(Self::Admin),
            other => Err(DomainError::InvalidArgument(format!(
                "unknown access level `{other}`"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Admin => "admin",
        }
    }
}

/// `level` on `prefix` (None = whole vault). The canonical string form is
/// `level:prefix`, e.g. `read:/`, `write:projects/rust-ci/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub level: AccessLevel,
    pub prefix: Option<String>,
}

impl Scope {
    /// Parse `level[:prefix]`. An empty prefix means the whole vault.
    pub fn parse(raw: &str) -> DomainResult<Self> {
        let (level_raw, prefix) = raw.split_once(':').unwrap_or((raw, ""));
        let level = AccessLevel::parse(level_raw)?;
        let prefix =
            path::normalize_prefix(Some(prefix.trim_start_matches('/'))).map(str::to_owned);
        Ok(Self { level, prefix })
    }

    pub fn as_string(&self) -> String {
        match &self.prefix {
            None => format!("{}:/", self.level.as_str()),
            Some(p) => format!("{}:{}", self.level.as_str(), p),
        }
    }

    /// Segment-boundary containment; `level` satisfied when the scope's level
    /// is at least as powerful as the requested one.
    pub fn allows(&self, level: AccessLevel, logical_path: &str) -> bool {
        if self.level < level {
            return false;
        }
        match &self.prefix {
            None => true,
            Some(p) => path::prefix_has(p, logical_path),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Principal {
    /// Server-derived identity written into revisions (e.g. `token:default`).
    pub actor: String,
    pub scopes: Vec<Scope>,
}

impl Principal {
    pub fn allows(&self, level: AccessLevel, logical_path: &str) -> bool {
        self.scopes.iter().any(|s| s.allows(level, logical_path))
    }

    pub fn require(&self, level: AccessLevel, logical_path: &str) -> DomainResult<()> {
        if self.allows(level, logical_path) {
            Ok(())
        } else {
            Err(DomainError::Forbidden(format!(
                "missing {} access for {logical_path}",
                level.as_str()
            )))
        }
    }

    /// True when the principal may read at least one path within `prefix`.
    pub fn any_read_within(&self, prefix: Option<&str>) -> bool {
        match prefix {
            // Whole-vault listing: any read scope grants visibility.
            None => self.scopes.iter().any(|s| s.level >= AccessLevel::Read),
            Some(p) => self.scopes.iter().any(|s| {
                s.level >= AccessLevel::Read
                    && s.prefix
                        .as_deref()
                        .is_none_or(|sp| path::prefix_has(p, sp) || path::prefix_has(sp, p))
            }),
        }
    }
}
