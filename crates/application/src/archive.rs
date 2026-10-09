//! Administrative archive restoration. Immutable objects first; each bounded
//! HEAD publication advances exactly one source generation with CAS.

use crate::{Vault, conflict};
use brains_domain::{
    AccessLevel, DomainError, DomainResult, Head, HexId, Principal, Revision, VaultPath,
};
use brains_ports::{BlobStore, CasOutcome, ExpectedHead, HeadStore, IndexStore, RevisionStore};
use sha2::{Digest, Sha256};

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: HeadStore + Send + Sync,
    R: RevisionStore + Send + Sync,
    B: BlobStore + Send + Sync,
    I: IndexStore + Send + Sync,
{
    pub async fn archive_revision(
        &self,
        principal: &Principal,
        path: &VaultPath,
        id: &HexId,
    ) -> DomainResult<Revision> {
        principal.require(AccessLevel::Admin, path.as_str())?;
        let revision = self
            .revisions
            .get(id)
            .await?
            .ok_or_else(|| DomainError::NotFound {
                path: path.to_string(),
            })?;
        revision.validate()?;
        if revision.revision != *id || revision.path != path.as_str() {
            return Err(DomainError::InvalidFormat(
                "archive revision identity/path mismatch".into(),
            ));
        }
        Ok(revision)
    }

    pub async fn import_revision(
        &self,
        principal: &Principal,
        source: &Revision,
    ) -> DomainResult<Revision> {
        source
            .validate()
            .map_err(|e| DomainError::InvalidFormat(e.to_string()))?;
        principal.require(AccessLevel::Admin, &source.path)?;
        let revision = source.in_vault(&self.vault)?;
        let metadata_size = serde_json::to_vec(&revision)
            .map_err(|e| DomainError::Internal(e.to_string()))?
            .len();
        if metadata_size > 65536 {
            return Err(DomainError::TooLarge {
                limit: 65536,
                actual: metadata_size,
            });
        }
        if let Some(existing) = self.revisions.get(&revision.revision).await? {
            return identical_revision(existing, &revision);
        }
        if let Err(error) = self.revisions.create(&revision).await {
            // A racing/retried create may have succeeded. No adapter error
            // strings are interpreted, and differing immutable data is fatal.
            return match self.revisions.get(&revision.revision).await? {
                Some(existing) => identical_revision(existing, &revision),
                None => Err(error),
            };
        }
        Ok(revision)
    }

    pub async fn import_blob(
        &self,
        principal: &Principal,
        path: &VaultPath,
        id: &HexId,
        bytes: &[u8],
    ) -> DomainResult<()> {
        let revision = self.archive_revision(principal, path, id).await?;
        let content = revision
            .content
            .as_ref()
            .ok_or_else(|| DomainError::InvalidArgument("tombstone has no blob".into()))?;
        if bytes.len() > self.limits.max_attachment_bytes {
            return Err(DomainError::TooLarge {
                limit: self.limits.max_attachment_bytes,
                actual: bytes.len(),
            });
        }
        verify_blob(content, bytes, path)?;
        match self.blobs.get(&content.key).await {
            Ok(existing) => return identical_blob(&existing, bytes, path),
            Err(DomainError::NotFound { .. }) => (),
            Err(error) => return Err(error),
        }
        if let Err(error) = self
            .blobs
            .create(&content.key, bytes, &content.sha256)
            .await
        {
            return match self.blobs.get(&content.key).await {
                Ok(existing) => identical_blob(&existing, bytes, path),
                Err(DomainError::NotFound { .. }) => Err(error),
                Err(read_error) => Err(read_error),
            };
        }
        Ok(())
    }

    /// Indexes are deliberately rebuilt once after the complete import. The
    /// caller must not switch normal use to this target during replay.
    pub async fn import_head(
        &self,
        principal: &Principal,
        path: &VaultPath,
        id: &HexId,
    ) -> DomainResult<Head> {
        let revision = self.archive_revision(principal, path, id).await?;
        let head = revision.head()?;
        if let Some(content) = &revision.content {
            let bytes = self.blobs.get(&content.key).await?;
            verify_blob(content, &bytes, path)?;
        }
        let current = self.heads.get(path).await?;
        let expected = match &current {
            Some(stored) if stored.head == head => return Ok(head),
            Some(stored)
                if stored.head.path == path.as_str()
                    && revision.parent.as_ref() == Some(&stored.head.revision)
                    && revision.generation == stored.head.generation + 1 =>
            {
                stored.head.validate()?;
                stored.expected()
            }
            None if revision.parent.is_none() && revision.generation == 1 => ExpectedHead::Absent,
            _ => {
                return Err(conflict(
                    path,
                    current.as_ref().map(|stored| stored.head.revision.as_str()),
                ));
            }
        };
        if self.heads.cas(path, &expected, &head).await? == CasOutcome::PreconditionFailed {
            let now = self.heads.get(path).await?;
            return Err(conflict(
                path,
                now.as_ref().map(|stored| stored.head.revision.as_str()),
            ));
        }
        Ok(head)
    }
}

fn identical_revision(existing: Revision, expected: &Revision) -> DomainResult<Revision> {
    if existing == *expected {
        Ok(existing)
    } else {
        Err(DomainError::RevisionConflict {
            path: expected.path.clone(),
            current_revision: Some(existing.revision.to_string()),
        })
    }
}

fn identical_blob(existing: &[u8], expected: &[u8], path: &VaultPath) -> DomainResult<()> {
    if existing == expected {
        Ok(())
    } else {
        Err(DomainError::ChecksumMismatch {
            path: path.to_string(),
        })
    }
}

fn verify_blob(
    content: &brains_domain::ContentRef,
    bytes: &[u8],
    path: &VaultPath,
) -> DomainResult<()> {
    if content.size == bytes.len() as i64 && content.sha256 == hex::encode(Sha256::digest(bytes)) {
        Ok(())
    } else {
        Err(DomainError::ChecksumMismatch {
            path: path.to_string(),
        })
    }
}
