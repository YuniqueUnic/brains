//! The single commit protocol (design §8.2, §22.2): put / trash / restore.

use sha2::{Digest, Sha256};

use brainstore_domain::{
    AccessLevel, DomainError, DomainResult, Head, HeadState, HexId, Principal, Revision,
    RevisionAction, VaultPath, blob_key,
};
use brainstore_ports::{CasOutcome, ExpectedHead};

use crate::projection;
use crate::{IndexState, MutationOutcome, Precondition, SizeClass, Vault, conflict};

pub struct CommitInput<'a> {
    pub path: &'a VaultPath,
    pub content: &'a [u8],
    /// `None` derives the media type from the path extension.
    pub media_type: Option<&'a str>,
    pub actor: String,
    pub operation_id: HexId,
    pub precondition: Precondition,
    pub executable: bool,
    /// `Some` turns the commit into a `copy` revision with this provenance.
    pub copied_from: Option<brainstore_domain::CopyProvenance>,
}

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brainstore_ports::HeadStore + Send + Sync,
    R: brainstore_ports::RevisionStore + Send + Sync,
    B: brainstore_ports::BlobStore + Send + Sync,
    I: brainstore_ports::IndexStore + Send + Sync,
{
    /// Create (201) or conditionally replace (200) one file's exact bytes.
    pub async fn put_file(
        &self,
        principal: &Principal,
        input: CommitInput<'_>,
    ) -> DomainResult<MutationOutcome> {
        self.require_scope(principal, AccessLevel::Write, input.path)?;
        // Text validation happens before CAS (design §22.2): declared-text
        // content that cannot be decoded never reaches the stores.
        if self.size_class(input.path, input.content)? == SizeClass::TooLarge {
            return Err(DomainError::TooLarge {
                limit: self.limits.max_attachment_bytes,
                actual: input.content.len(),
            });
        }

        let current = self
            .check_precondition(input.path, &input.precondition)
            .await?;
        let created = current.is_none();
        let (generation, parent) = match &current {
            Some(stored) => (
                stored.head.generation + 1,
                Some(stored.head.revision.clone()),
            ),
            None => (1, None),
        };

        let revision_id = HexId::generate();
        let key = blob_key(&self.vault, &revision_id, input.path.file_name());
        let sha256 = hex::encode(Sha256::digest(input.content));
        self.blobs.create(&key, input.content, &sha256).await?;

        let media_type = input
            .media_type
            .map(str::to_owned)
            .unwrap_or_else(|| brainstore_domain::media_type_for(input.path.as_str()).to_owned());
        let action = if input.copied_from.is_some() {
            RevisionAction::Copy
        } else {
            RevisionAction::Put
        };
        let revision = Revision::new(
            revision_id,
            input.path,
            generation,
            parent,
            action,
            input.actor.clone(),
            input.operation_id.clone(),
            brainstore_domain::timestamp::now_rfc3339(),
            None,
            input.copied_from,
            Some(brainstore_domain::ContentRef {
                key: key.clone(),
                sha256: sha256.clone(),
                size: input.content.len() as i64,
                media_type,
                executable: input.executable,
            }),
        )?;
        self.revisions.create(&revision).await?;

        let head = Head::new(
            input.path,
            revision.revision.clone(),
            generation,
            HeadState::Live,
            revision.created_at.clone(),
        )?;
        let expected = match &current {
            Some(stored) => stored.expected(),
            None => ExpectedHead::Absent,
        };
        match self.heads.cas(input.path, &expected, &head).await? {
            CasOutcome::Committed => {}
            CasOutcome::PreconditionFailed => {
                // Another writer won; its objects stay, ours are orphaned
                // candidates (design §8.4). Re-read for the conflict body.
                let now_current = self.current_head(input.path).await?;
                return Err(conflict(
                    input.path,
                    now_current.as_ref().map(|s| s.head.revision.as_str()),
                ));
            }
        }

        let index_state = self
            .project_committed(&revision, Some(input.content), false)
            .await;
        Ok(MutationOutcome {
            path: input.path.as_str().to_owned(),
            revision: revision.revision.as_str().to_owned(),
            generation,
            updated_at: revision.created_at.clone(),
            size: input.content.len() as i64,
            sha256,
            media_type: revision
                .content
                .as_ref()
                .expect("put has content")
                .media_type
                .clone(),
            kind: brainstore_domain::detect_kind(input.path.as_str(), input.content),
            created,
            index_state,
            operation_id: input.operation_id.as_str().to_owned(),
        })
    }

    /// Soft delete: a tombstone revision, HEAD CAS, keep all history
    /// (design §8.6). Trashing a tombstone is a 410, not a no-op version.
    pub async fn trash_file(
        &self,
        principal: &Principal,
        path: &VaultPath,
        precondition: Precondition,
        actor: String,
        operation_id: HexId,
    ) -> DomainResult<MutationOutcome> {
        self.require_scope(principal, AccessLevel::Write, path)?;
        let current = self.check_precondition(path, &precondition).await?;
        let Some(stored) = current else {
            return Err(DomainError::NotFound {
                path: path.as_str().to_owned(),
            });
        };
        if stored.head.state == HeadState::Trashed {
            return Err(DomainError::Trashed {
                path: path.as_str().to_owned(),
                revision: stored.head.revision.as_str().to_owned(),
            });
        }
        let generation = stored.head.generation + 1;
        let revision = Revision::new(
            HexId::generate(),
            path,
            generation,
            Some(stored.head.revision.clone()),
            RevisionAction::Trash,
            actor,
            operation_id,
            brainstore_domain::timestamp::now_rfc3339(),
            None,
            None,
            None,
        )?;
        self.revisions.create(&revision).await?;
        let head = Head::new(
            path,
            revision.revision.clone(),
            generation,
            HeadState::Trashed,
            revision.created_at.clone(),
        )?;
        match self.heads.cas(path, &stored.expected(), &head).await? {
            CasOutcome::Committed => {}
            CasOutcome::PreconditionFailed => {
                let now_current = self.current_head(path).await?;
                return Err(conflict(
                    path,
                    now_current.as_ref().map(|s| s.head.revision.as_str()),
                ));
            }
        }
        let index_state = self.project_committed(&revision, None, true).await;
        Ok(MutationOutcome {
            path: path.as_str().to_owned(),
            revision: revision.revision.as_str().to_owned(),
            generation,
            updated_at: revision.created_at.clone(),
            size: 0,
            sha256: String::new(),
            media_type: brainstore_domain::media_type_for(path.as_str()).to_owned(),
            kind: brainstore_domain::detect_kind(path.as_str(), b""),
            created: false,
            index_state,
            operation_id: revision.operation_id.as_str().to_owned(),
        })
    }

    /// Restore prior content as a NEW revision (design §8.1): never rewind
    /// HEAD, never lose the tombstone, reuse the immutable blob.
    pub async fn restore_file(
        &self,
        principal: &Principal,
        path: &VaultPath,
        source_revision: &HexId,
        precondition: Precondition,
        actor: String,
        operation_id: HexId,
    ) -> DomainResult<MutationOutcome> {
        self.require_scope(principal, AccessLevel::Write, path)?;
        let current = self.check_precondition(path, &precondition).await?;
        let Some(stored) = current else {
            return Err(DomainError::NotFound {
                path: path.as_str().to_owned(),
            });
        };
        let source =
            self.revisions
                .get(source_revision)
                .await?
                .ok_or_else(|| DomainError::NotFound {
                    path: format!("revision {source_revision}"),
                })?;
        if source.path != path.as_str() {
            return Err(DomainError::NotFound {
                path: format!("revision {source_revision}"),
            });
        }
        let content = source.content.clone().ok_or_else(|| {
            DomainError::InvalidArgument("cannot restore a tombstone revision".into())
        })?;
        // A restore must not commit a missing or corrupt blob.
        let bytes = self.blobs.get(&content.key).await?;
        if bytes.len() as i64 != content.size
            || hex::encode(Sha256::digest(&bytes)) != content.sha256
        {
            return Err(DomainError::StorageUnavailable(format!(
                "restore source blob is missing or corrupt: {}",
                content.key
            )));
        }
        let generation = stored.head.generation + 1;
        let revision = Revision::new(
            HexId::generate(),
            path,
            generation,
            Some(stored.head.revision.clone()),
            RevisionAction::Restore,
            actor,
            operation_id,
            brainstore_domain::timestamp::now_rfc3339(),
            Some(source.revision.clone()),
            None,
            Some(content.clone()),
        )?;
        self.revisions.create(&revision).await?;
        let head = Head::new(
            path,
            revision.revision.clone(),
            generation,
            HeadState::Live,
            revision.created_at.clone(),
        )?;
        match self.heads.cas(path, &stored.expected(), &head).await? {
            CasOutcome::Committed => {}
            CasOutcome::PreconditionFailed => {
                let now_current = self.current_head(path).await?;
                return Err(conflict(
                    path,
                    now_current.as_ref().map(|s| s.head.revision.as_str()),
                ));
            }
        }
        let index_state = self.project_committed(&revision, Some(&bytes), false).await;
        Ok(MutationOutcome {
            path: path.as_str().to_owned(),
            revision: revision.revision.as_str().to_owned(),
            generation,
            updated_at: revision.created_at.clone(),
            size: content.size,
            sha256: content.sha256.clone(),
            media_type: content.media_type.clone(),
            kind: brainstore_domain::detect_kind(path.as_str(), &bytes),
            created: false,
            index_state,
            operation_id: revision.operation_id.as_str().to_owned(),
        })
    }

    /// Index projection of an already-committed revision (design §8.2 step 7):
    /// failure leaves the commit in place and reports `pending`.
    async fn project_committed(
        &self,
        revision: &Revision,
        content: Option<&[u8]>,
        deleted: bool,
    ) -> IndexState {
        let proj = projection::build_projection(
            &self.vault,
            revision,
            content,
            deleted,
            self.limits.max_text_bytes,
        );
        match self.index.upsert(&proj).await {
            Ok(()) => IndexState::Ready,
            Err(_) => IndexState::Pending,
        }
    }
}
