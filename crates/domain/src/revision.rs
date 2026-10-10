//! Immutable revision records (design §7.1). A revision is written once and
//! never modified; trash revisions carry `content: null`; restore revisions
//! point at the reused immutable blob of the restored source.
//! Serialized form matches contracts/revision.schema.json.

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};
use crate::head::MAX_GENERATION;
use crate::id::HexId;
use crate::path::VaultPath;
use crate::timestamp::validate_rfc3339;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RevisionAction {
    Put,
    Trash,
    Restore,
    Copy,
}

impl RevisionAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Put => "put",
            Self::Trash => "trash",
            Self::Restore => "restore",
            Self::Copy => "copy",
        }
    }
}

/// Reference to the immutable blob holding this revision's bytes.
/// `key` is a storage-internal object key, not a URL:
/// `vaults/{vault}/blobs/{revision}/{file_name}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentRef {
    pub key: String,
    pub sha256: String,
    pub size: i64,
    pub media_type: String,
    pub executable: bool,
}

/// Provenance recorded on `copy` revisions (design §8.7). The target keeps its
/// own history; this field documents where the content came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyProvenance {
    pub path: String,
    pub revision: HexId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub schema_version: u64,
    pub revision: HexId,
    pub path: String,
    pub generation: i64,
    pub parent: Option<HexId>,
    pub action: RevisionAction,
    pub actor: String,
    pub operation_id: HexId,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restores_revision: Option<HexId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copied_from: Option<CopyProvenance>,
    pub content: Option<ContentRef>,
}

impl Revision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        revision: HexId,
        path: &VaultPath,
        generation: i64,
        parent: Option<HexId>,
        action: RevisionAction,
        actor: String,
        operation_id: HexId,
        created_at: String,
        restores_revision: Option<HexId>,
        copied_from: Option<CopyProvenance>,
        content: Option<ContentRef>,
    ) -> DomainResult<Self> {
        let rev = Self {
            schema_version: crate::head::SCHEMA_VERSION,
            revision,
            path: path.as_str().to_owned(),
            generation,
            parent,
            action,
            actor,
            operation_id,
            created_at,
            restores_revision,
            copied_from,
            content,
        };
        rev.validate()?;
        Ok(rev)
    }

    /// Cross-field invariants enforced on every load and before every write.
    pub fn validate(&self) -> DomainResult<()> {
        if self.schema_version != crate::head::SCHEMA_VERSION {
            return Err(DomainError::Internal(format!(
                "unsupported revision schema_version {}",
                self.schema_version
            )));
        }
        let path = VaultPath::new(&self.path)?;
        if self.generation < 1 || self.generation > MAX_GENERATION {
            return Err(DomainError::Internal(format!(
                "revision generation {} out of range",
                self.generation
            )));
        }
        if self.parent.is_none() != (self.generation == 1) {
            return Err(DomainError::Internal(
                "parent must be absent exactly at generation 1".into(),
            ));
        }
        if self.parent.as_ref() == Some(&self.revision) {
            return Err(DomainError::Internal("revision cycle".into()));
        }
        if self.actor.is_empty() {
            return Err(DomainError::Internal("actor must not be empty".into()));
        }
        validate_rfc3339(&self.created_at)?;
        match self.action {
            RevisionAction::Trash => {
                if self.content.is_some() {
                    return Err(DomainError::Internal("trash revision has content".into()));
                }
                if self.restores_revision.is_some() || self.copied_from.is_some() {
                    return Err(DomainError::Internal(
                        "trash revision carries provenance".into(),
                    ));
                }
            }
            RevisionAction::Restore => {
                if self.restores_revision.is_none() {
                    return Err(DomainError::Internal(
                        "restore revision without restores_revision".into(),
                    ));
                }
                Self::require_content(self.content.as_ref(), &path)?;
            }
            RevisionAction::Copy => {
                if self.copied_from.is_none() {
                    return Err(DomainError::Internal(
                        "copy revision without copied_from".into(),
                    ));
                }
                Self::require_content(self.content.as_ref(), &path)?;
            }
            RevisionAction::Put => {
                if self.restores_revision.is_some() || self.copied_from.is_some() {
                    return Err(DomainError::Internal(
                        "put revision carries provenance".into(),
                    ));
                }
                Self::require_content(self.content.as_ref(), &path)?;
            }
        }
        Ok(())
    }

    fn require_content(content: Option<&ContentRef>, path: &VaultPath) -> DomainResult<()> {
        let content = content.ok_or_else(|| {
            DomainError::Internal(format!("non-trash revision of {path} missing content"))
        })?;
        if content.sha256.len() != 64
            || !content
                .sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(DomainError::Internal(
                "content sha256 must be 64 lowercase hex".into(),
            ));
        }
        if content.size < 0 || content.size > MAX_GENERATION {
            return Err(DomainError::Internal("content size out of range".into()));
        }
        validate_blob_key(&content.key, path.file_name())?;
        if content.media_type.is_empty() {
            return Err(DomainError::Internal("media_type must not be empty".into()));
        }
        Ok(())
    }

    pub fn path(&self) -> VaultPath {
        VaultPath::trusted(self.path.clone())
    }

    /// Archive transfer preserves identity, changing only the storage namespace.
    pub fn in_vault(&self, vault: &str) -> DomainResult<Self> {
        self.validate()?;
        let mut revision = self.clone();
        if let Some(content) = &mut revision.content {
            let object = content
                .key
                .splitn(4, '/')
                .nth(3)
                .ok_or_else(|| DomainError::InvalidFormat("invalid archive blob key".into()))?;
            content.key = format!("vaults/{vault}/blobs/{object}");
        }
        revision.validate()?;
        Ok(revision)
    }

    pub fn head(&self) -> DomainResult<crate::Head> {
        crate::Head::new(
            &self.path(),
            self.revision.clone(),
            self.generation,
            if self.action == RevisionAction::Trash {
                crate::HeadState::Trashed
            } else {
                crate::HeadState::Live
            },
            self.created_at.clone(),
        )
    }
}

/// Blob keys live under `vaults/{vault}/blobs/{revision-id}/{file_name}` and
/// the file name must equal the logical path's file name.
pub fn validate_blob_key(key: &str, expected_file_name: &str) -> DomainResult<()> {
    let invalid = || DomainError::Internal(format!("malformed blob key `{key}`"));
    let rest = key
        .strip_prefix("vaults/")
        .ok_or_else(invalid)?
        .strip_suffix(&format!("/{expected_file_name}"))
        .ok_or_else(invalid)?;
    let mut parts = rest.split('/');
    let vault = parts.next().ok_or_else(invalid)?;
    if vault.is_empty()
        || !vault
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        || vault.len() > 32
    {
        return Err(invalid());
    }
    if parts.next() != Some("blobs") {
        return Err(invalid());
    }
    HexId::from_hex(parts.next().ok_or_else(invalid)?).map_err(|_| invalid())?;
    if parts.next().is_some() {
        return Err(invalid());
    }
    if expected_file_name.is_empty() || expected_file_name.contains('/') {
        return Err(invalid());
    }
    Ok(())
}

/// Build the canonical blob key for a revision's content.
pub fn blob_key(vault: &str, revision: &HexId, file_name: &str) -> String {
    format!("vaults/{vault}/blobs/{revision}/{file_name}")
}
