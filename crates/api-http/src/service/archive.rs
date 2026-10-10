//! Administrative archive transport. The wire DTO omits absent optionals;
//! immutable domain records retain their storage representation and validation.

use super::*;

pub(crate) struct ArchiveService<H: Head, R: Rev, B: Blob, I: Idx> {
    pub state: SharedState<H, R, B, I>,
}

error_from_domain!(
    archive_revision_error,
    ArchivesArchiveRevisionError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        UnprocessableEntity,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    import_revision_error,
    ArchivesImportArchiveRevisionError,
    [
        BadRequest,
        Forbidden,
        PreconditionFailed,
        PayloadTooLarge,
        UnprocessableEntity,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    import_blob_error,
    ArchivesImportArchiveBlobError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        PayloadTooLarge,
        UnprocessableEntity,
        ServiceUnavailable,
        Internal
    ]
);
error_from_domain!(
    import_head_error,
    ArchivesImportArchiveHeadError,
    [
        BadRequest,
        Forbidden,
        NotFound,
        PreconditionFailed,
        UnprocessableEntity,
        ServiceUnavailable,
        Internal
    ]
);

impl From<brains_domain::Revision> for contract::ArchiveRevision {
    fn from(value: brains_domain::Revision) -> Self {
        Self {
            schema_version: value.schema_version,
            revision: value.revision.to_string(),
            path: value.path,
            generation: value.generation,
            parent: value.parent.map(|id| id.to_string()),
            action: value.action.as_str().to_owned(),
            actor: value.actor,
            operation_id: value.operation_id.to_string(),
            created_at: value.created_at,
            restores_revision: value.restores_revision.map(|id| id.to_string()),
            copied_from: value.copied_from.map(|copy| contract::CopiedFrom {
                path: copy.path,
                revision: copy.revision.to_string(),
            }),
            content: value.content.map(|content| contract::ArchiveContent {
                key: content.key,
                sha256: content.sha256,
                size: content.size,
                media_type: content.media_type,
                executable: content.executable,
            }),
        }
    }
}

impl TryFrom<contract::ArchiveRevision> for brains_domain::Revision {
    type Error = DomainError;

    fn try_from(value: contract::ArchiveRevision) -> Result<Self, Self::Error> {
        use brains_domain::revision::RevisionAction;
        Ok(Self {
            schema_version: value.schema_version,
            revision: HexId::from_hex(&value.revision)?,
            path: value.path,
            generation: value.generation,
            parent: value.parent.as_deref().map(HexId::from_hex).transpose()?,
            action: match value.action.as_str() {
                "put" => RevisionAction::Put,
                "trash" => RevisionAction::Trash,
                "restore" => RevisionAction::Restore,
                "copy" => RevisionAction::Copy,
                _ => {
                    return Err(DomainError::InvalidArgument(
                        "invalid archive action".into(),
                    ));
                }
            },
            actor: value.actor,
            operation_id: HexId::from_hex(&value.operation_id)?,
            created_at: value.created_at,
            restores_revision: value
                .restores_revision
                .as_deref()
                .map(HexId::from_hex)
                .transpose()?,
            copied_from: value
                .copied_from
                .map(|copy| {
                    Ok(brains_domain::revision::CopyProvenance {
                        path: copy.path,
                        revision: HexId::from_hex(&copy.revision)?,
                    })
                })
                .transpose()?,
            content: value
                .content
                .map(|content| brains_domain::revision::ContentRef {
                    key: content.key,
                    sha256: content.sha256,
                    size: content.size,
                    media_type: content.media_type,
                    executable: content.executable,
                }),
        })
    }
}

#[async_trait::async_trait]
impl<H: Head, R: Rev, B: Blob, I: Idx> contract::Archives for ArchiveService<H, R, B, I> {
    async fn archive_revision(
        &self,
        path: String,
        revision: String,
    ) -> Result<contract::ArchiveRevision, contract::ArchivesArchiveRevisionError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| archive_revision_error(e, rid))?;
        let revision = HexId::from_hex(&revision).map_err(|e| archive_revision_error(e, rid))?;
        self.state
            .vault
            .archive_revision(ctx.auth.principal(), &path, &revision)
            .await
            .map(Into::into)
            .map_err(|e| archive_revision_error(e, rid))
    }

    async fn import_archive_revision(
        &self,
        body: contract::ArchiveRevision,
    ) -> Result<contract::ArchiveRevision, contract::ArchivesImportArchiveRevisionError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let revision = body.try_into().map_err(|e| import_revision_error(e, rid))?;
        self.state
            .vault
            .import_revision(ctx.auth.principal(), &revision)
            .await
            .map(Into::into)
            .map_err(|e| import_revision_error(e, rid))
    }

    async fn import_archive_blob(
        &self,
        path: String,
        revision: String,
        body: Vec<u8>,
    ) -> Result<contract::ArchiveBlobStored, contract::ArchivesImportArchiveBlobError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| import_blob_error(e, rid))?;
        let revision = HexId::from_hex(&revision).map_err(|e| import_blob_error(e, rid))?;
        self.state
            .vault
            .import_blob(ctx.auth.principal(), &path, &revision, &body)
            .await
            .map_err(|e| import_blob_error(e, rid))?;
        Ok(contract::ArchiveBlobStored { stored: true })
    }

    async fn import_archive_head(
        &self,
        path: String,
        revision: String,
    ) -> Result<contract::ArchivePublished, contract::ArchivesImportArchiveHeadError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| import_head_error(e, rid))?;
        let revision = HexId::from_hex(&revision).map_err(|e| import_head_error(e, rid))?;
        let head = self
            .state
            .vault
            .import_head(ctx.auth.principal(), &path, &revision)
            .await
            .map_err(|e| import_head_error(e, rid))?;
        Ok(contract::ArchivePublished {
            head: contract::ArchiveHead {
                schema_version: head.schema_version,
                path: head.path,
                revision: head.revision.to_string(),
                generation: head.generation,
                state: head.state.as_str().to_owned(),
                updated_at: head.updated_at,
            },
            index_state: "pending".into(),
        })
    }
}
