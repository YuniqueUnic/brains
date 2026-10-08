//! Copy/move (design §8.7, P1): never an atomic rename. Copy creates the
//! target create-only with provenance; move then conditionally trashes an
//! unchanged source. A source that moved mid-operation yields a partial
//! result both sides keep, never a rollback of the user's target.

use brainstore_domain::{
    AccessLevel, CopyProvenance, DomainError, DomainResult, HexId, Principal, VaultPath,
};

use crate::commit::CommitInput;
use crate::{MutationOutcome, Precondition, Vault};

pub struct TransferInput {
    pub source: VaultPath,
    pub target: VaultPath,
    pub expected_source_revision: HexId,
    pub actor: String,
    pub operation_id: HexId,
}

pub struct TransferOutcome {
    pub copied: bool,
    pub source_trashed: bool,
    pub partial: bool,
    pub operation_id: String,
    pub target_revision: Option<String>,
    pub source_revision: Option<String>,
    pub error: Option<DomainError>,
}

impl<H, R, B, I> Vault<H, R, B, I>
where
    H: brainstore_ports::HeadStore + Send + Sync,
    R: brainstore_ports::RevisionStore + Send + Sync,
    B: brainstore_ports::BlobStore + Send + Sync,
    I: brainstore_ports::IndexStore + Send + Sync,
{
    pub async fn copy_file(
        &self,
        principal: &Principal,
        input: TransferInput,
    ) -> DomainResult<TransferOutcome> {
        self.require_scope(principal, AccessLevel::Write, &input.source)?;
        self.require_scope(principal, AccessLevel::Write, &input.target)?;
        let copy = self.copy_step(principal, &input).await;
        match copy {
            Ok(outcome) => Ok(TransferOutcome {
                copied: true,
                source_trashed: false,
                partial: false,
                operation_id: input.operation_id.as_str().to_owned(),
                target_revision: Some(outcome.revision),
                source_revision: Some(input.expected_source_revision.as_str().to_owned()),
                error: None,
            }),
            Err(e) => Err(e),
        }
    }

    pub async fn move_file(
        &self,
        principal: &Principal,
        input: TransferInput,
    ) -> DomainResult<TransferOutcome> {
        self.require_scope(principal, AccessLevel::Write, &input.source)?;
        self.require_scope(principal, AccessLevel::Write, &input.target)?;
        let copied = self.copy_step(principal, &input).await;
        let target_revision = match &copied {
            Ok(outcome) => Some(outcome.revision.clone()),
            Err(e) => return Err(e.clone()),
        };
        // Trash the source only if it still sits at the expected revision.
        let trash = self
            .trash_file(
                principal,
                &input.source,
                Precondition::IfRevision(input.expected_source_revision.clone()),
                input.actor.clone(),
                input.operation_id.clone(),
            )
            .await;
        match trash {
            Ok(outcome) => Ok(TransferOutcome {
                copied: true,
                source_trashed: true,
                partial: false,
                operation_id: input.operation_id.as_str().to_owned(),
                target_revision,
                source_revision: Some(outcome.revision),
                error: None,
            }),
            Err(e) => {
                // The target stays; the source stays; the caller repairs.
                Ok(TransferOutcome {
                    copied: true,
                    source_trashed: false,
                    partial: true,
                    operation_id: input.operation_id.as_str().to_owned(),
                    target_revision,
                    source_revision: None,
                    error: Some(e),
                })
            }
        }
    }

    /// Create-only copy of the source's live bytes to the target path.
    async fn copy_step(
        &self,
        principal: &Principal,
        input: &TransferInput,
    ) -> DomainResult<MutationOutcome> {
        let source_head =
            self.current_head(&input.source)
                .await?
                .ok_or_else(|| DomainError::NotFound {
                    path: input.source.as_str().to_owned(),
                })?;
        if source_head.head.state != brainstore_domain::HeadState::Live {
            return Err(DomainError::Trashed {
                path: input.source.as_str().to_owned(),
                revision: source_head.head.revision.as_str().to_owned(),
            });
        }
        if source_head.head.revision != input.expected_source_revision {
            return Err(crate::conflict(
                &input.source,
                Some(source_head.head.revision.as_str()),
            ));
        }
        let revision = self.load_revision(&source_head.head.revision).await?;
        let bytes = self.revision_bytes(&revision).await?;
        let content_ref = revision.content.clone().expect("live revision has content");
        self.put_file(
            principal,
            CommitInput {
                path: &input.target,
                content: &bytes,
                media_type: Some(&content_ref.media_type),
                actor: input.actor.clone(),
                operation_id: input.operation_id.clone(),
                precondition: Precondition::Create,
                executable: content_ref.executable,
                copied_from: Some(CopyProvenance {
                    path: input.source.as_str().to_owned(),
                    revision: input.expected_source_revision.clone(),
                }),
            },
        )
        .await
    }
}
