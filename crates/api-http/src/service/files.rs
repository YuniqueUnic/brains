//! The `Files` trait: every store operation from listing through reindex.
//! Failures classify via `service::error`; request plumbing lives in
//! `service::conditional`; payload shapes in `service::dto`.

use brains_application::Precondition;
use brains_application::commit::CommitInput;
use brains_domain::{DomainError, HexId, VaultPath};

use crate::context::current;
use crate::contract::brains as contract;
use crate::state::{Blob, Head, Idx, Rev, SharedState};

use super::conditional::{
    clamp_limit, not_modified, parse_operation_id, raw_headers, required_if_match,
    validate_declared_sha, write_precondition,
};
use super::dto::{
    capabilities as capabilities_dto, entry, media_type_or_default, mutation_result, quoted_etag,
    revision_dto,
};
use super::error::{head_file_error, *};
use super::transfer::{TransferKind, TransferOutcome};

// -- Services ---------------------------------------------------------------

pub struct FilesService<H: Head, R: Rev, B: Blob, I: Idx> {
    pub state: SharedState<H, R, B, I>,
}

impl<H: Head, R: Rev, B: Blob, I: Idx> Clone for FilesService<H, R, B, I> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

#[async_trait::async_trait]
impl<H: Head, R: Rev, B: Blob, I: Idx> contract::Files for FilesService<H, R, B, I> {
    async fn capabilities(&self) -> ::xidl_rust_axum::Result<contract::Capabilities> {
        Ok(capabilities_dto(&self.state))
    }

    async fn list_files(
        &self,
        body: contract::ListFilesQuery,
    ) -> Result<contract::FilePage, contract::FilesListFilesError> {
        let contract::ListFilesQuery {
            source,
            prefix,
            limit,
            cursor,
            tags,
            kind,
            sort,
            include_deleted,
            status,
            r#type,
            trust,
            stale,
        } = body;
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let query = brains_application::listing::ListQuery {
            source: source
                .as_deref()
                .unwrap_or("store")
                .parse()
                .map_err(|e| list_files_error(e, rid))?,
            prefix,
            limit: clamp_limit(
                limit,
                self.state.limits.list_page_default,
                self.state.limits.list_page_max,
            )
            .map_err(|e| list_files_error(e, rid))?,
            cursor,
            tags: tags.unwrap_or_default(),
            kind: kind.as_deref().and_then(brains_domain::FileKind::parse),
            status,
            type_: r#type,
            trust,
            stale,
            sort: if sort.as_deref() == Some("updated_desc") {
                brains_ports::ListOrder::UpdatedDesc
            } else {
                brains_ports::ListOrder::Path
            },
            include_deleted: include_deleted.unwrap_or(false),
        };
        let page = self
            .state
            .vault
            .list_files(ctx.auth.principal(), query)
            .await
            .map_err(|e| list_files_error(e, rid))?;
        Ok(contract::FilePage {
            items: page.items.iter().map(entry).collect(),
            next_cursor: page.next_cursor,
            source: page.source.to_string(),
            consistency: "per_file".into(),
        })
    }

    async fn put_file(
        &self,
        path: String,
        if_match: Option<String>,
        if_none_match: Option<String>,
        operation_id: Option<String>,
        content_sha256: Option<String>,
        executable: Option<bool>,
        content_type: Option<String>,
        body: Vec<u8>,
    ) -> Result<contract::FilesPutFileResponse, contract::FilesPutFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| put_file_error(e, rid))?;
        let precondition = write_precondition(if_match.as_deref(), if_none_match.as_deref())
            .map_err(|e| put_file_error(e, rid))?;
        let op_id =
            parse_operation_id(operation_id.as_ref()).map_err(|e| put_file_error(e, rid))?;
        let media_type = content_type.map(|value| value.trim().to_owned());
        let effective_media = media_type
            .clone()
            .unwrap_or_else(|| brains_domain::media_type_for(path.as_str()).to_owned());
        // Binary uploads must carry the digest header (§22.3); the bytes are
        // always verified against it before HEAD CAS.
        let declared_sha = content_sha256.as_deref();
        validate_declared_sha(
            declared_sha,
            brains_domain::is_text_media(&effective_media),
            &body,
            &path,
        )
        .map_err(|e| put_file_error(e, rid))?;
        let outcome = self
            .state
            .vault
            .put_file(
                ctx.auth.principal(),
                CommitInput {
                    path: &path,
                    content: &body,
                    media_type: media_type.as_deref(),
                    actor: ctx.auth.principal().actor.clone(),
                    operation_id: op_id,
                    precondition,
                    executable,
                    copied_from: None,
                },
            )
            .await
            .map_err(|e| put_file_error(e, rid))?;
        Ok(contract::FilesPutFileResponse {
            r#return: mutation_result(&outcome),
            etag: Some(quoted_etag(outcome.revision)),
        })
    }

    async fn get_file(
        &self,
        path: String,
        format: Option<String>,
        if_none_match: Option<String>,
    ) -> Result<contract::FilesGetFileResponse, contract::FilesGetFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| get_file_error(e, rid))?;
        let head = self
            .state
            .vault
            .read_entry(ctx.auth.principal(), &path)
            .await
            .map_err(|e| get_file_error(e, rid))?;
        let etag = quoted_etag(&head.revision);
        if not_modified(if_none_match.as_deref(), &etag) {
            return Err(contract::FilesGetFileError::NotModified(
                contract::NotModified { etag: etag.clone() },
            ));
        }
        if format.as_deref() == Some("meta") {
            return Ok(contract::FilesGetFileResponse {
                r#return: contract::FilePayload::Json(entry(&head)),
                etag: Some(etag),
                media_type: None,
                content_disposition: None,
            });
        }
        let content = self
            .state
            .vault
            .read_file(ctx.auth.principal(), &path)
            .await
            .map_err(|e| get_file_error(e, rid))?;
        let media = media_type_or_default(content.revision.content.as_ref());
        let (media_type, content_disposition) = raw_headers(&media, path.file_name());
        Ok(contract::FilesGetFileResponse {
            r#return: contract::FilePayload::OctetStream(content.bytes),
            etag: Some(etag),
            media_type,
            content_disposition,
        })
    }

    async fn head_file(
        &self,
        path: String,
    ) -> Result<contract::FilesHeadFileResponse, contract::FilesHeadFileError> {
        let ctx = current();
        let path = VaultPath::new(&path).map_err(head_file_error)?;
        let head = self
            .state
            .vault
            .read_entry(ctx.auth.principal(), &path)
            .await
            .map_err(head_file_error)?;
        Ok(contract::FilesHeadFileResponse {
            etag: quoted_etag(head.revision),
            generation: head.generation,
            state: head.state.as_str().to_owned(),
        })
    }

    async fn delete_file(
        &self,
        path: String,
        if_match: Option<String>,
        operation_id: Option<String>,
    ) -> Result<contract::FilesDeleteFileResponse, contract::FilesDeleteFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| delete_file_error(e, rid))?;
        let if_match =
            required_if_match(if_match.as_deref()).map_err(|e| delete_file_error(e, rid))?;
        let op_id =
            parse_operation_id(operation_id.as_ref()).map_err(|e| delete_file_error(e, rid))?;
        let outcome = self
            .state
            .vault
            .trash_file(
                ctx.auth.principal(),
                &path,
                Precondition::IfRevision(if_match),
                ctx.auth.principal().actor.clone(),
                op_id,
            )
            .await
            .map_err(|e| delete_file_error(e, rid))?;
        Ok(contract::FilesDeleteFileResponse {
            r#return: mutation_result(&outcome),
            etag: Some(quoted_etag(outcome.revision)),
        })
    }

    async fn purge_file(
        &self,
        path: String,
        if_match: Option<String>,
        operation_id: Option<String>,
    ) -> Result<contract::PurgeResult, contract::FilesPurgeFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| purge_file_error(e, rid))?;
        let revision =
            required_if_match(if_match.as_deref()).map_err(|e| purge_file_error(e, rid))?;
        let operation_id =
            parse_operation_id(operation_id.as_ref()).map_err(|e| purge_file_error(e, rid))?;
        let outcome = self
            .state
            .vault
            .purge_file(
                ctx.auth.principal(),
                &path,
                Precondition::IfRevision(revision),
                operation_id,
            )
            .await
            .map_err(|e| purge_file_error(e, rid))?;
        Ok(contract::PurgeResult {
            path: outcome.path,
            purged: true,
            index_state: outcome.index_state.as_str().to_owned(),
            operation_id: outcome.operation_id,
        })
    }

    async fn patch_file(
        &self,
        path: String,
        if_match: Option<String>,
        operation_id: Option<String>,
        body: contract::PatchBody,
    ) -> Result<contract::FilesPatchFileResponse, contract::FilesPatchFileError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| patch_file_error(e, rid))?;
        let if_match =
            required_if_match(if_match.as_deref()).map_err(|e| patch_file_error(e, rid))?;
        let op_id =
            parse_operation_id(operation_id.as_ref()).map_err(|e| patch_file_error(e, rid))?;
        let edits = body
            .edits
            .into_iter()
            .map(|edit| {
                let expected_matches = usize::try_from(edit.expected_matches).map_err(|_| {
                    DomainError::InvalidArgument("expected_matches must be nonnegative".into())
                })?;
                Ok(brains_application::patch::PatchEdit {
                    old_text: edit.old_text,
                    new_text: edit.new_text,
                    expected_matches,
                })
            })
            .collect::<Result<Vec<_>, DomainError>>()
            .map_err(|e| patch_file_error(e, rid))?;
        let outcome = self
            .state
            .vault
            .patch_file(
                ctx.auth.principal(),
                &path,
                Precondition::IfRevision(if_match),
                ctx.auth.principal().actor.clone(),
                op_id,
                brains_application::patch::PatchInput {
                    edits,
                    dry_run: body.dry_run.unwrap_or(false),
                },
            )
            .await
            .map_err(|e| patch_file_error(e, rid))?;
        let committed = outcome.committed.as_ref().map(mutation_result);
        let etag = committed
            .as_ref()
            .map(|result| quoted_etag(&result.revision));
        Ok(contract::FilesPatchFileResponse {
            r#return: contract::PatchResult {
                diff: outcome.diff,
                base_revision: outcome.base_revision,
                committed,
            },
            etag,
        })
    }

    async fn list_revisions(
        &self,
        path: String,
        limit: Option<i64>,
        cursor: Option<String>,
    ) -> Result<contract::HistoryPage, contract::FilesListRevisionsError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| list_revisions_error(e, rid))?;
        let limit = clamp_limit(
            limit,
            self.state.limits.history_page_max,
            self.state.limits.history_page_max,
        )
        .map_err(|e| list_revisions_error(e, rid))?;
        let page = self
            .state
            .vault
            .history_page(ctx.auth.principal(), &path, cursor.as_deref(), limit)
            .await
            .map_err(|e| list_revisions_error(e, rid))?;
        Ok(contract::HistoryPage {
            items: page
                .items
                .iter()
                .map(|rev| revision_dto(rev, "reachable"))
                .collect(),
            anchor_revision: page.anchor_revision,
            next_cursor: page.next_cursor,
        })
    }

    async fn get_revision(
        &self,
        path: String,
        revision: String,
        format: Option<String>,
    ) -> Result<contract::FilesGetRevisionResponse, contract::FilesGetRevisionError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| get_revision_error(e, rid))?;
        let revision_id = HexId::from_hex(&revision).map_err(|e| get_revision_error(e, rid))?;
        let record = self
            .state
            .vault
            .read_revision(ctx.auth.principal(), &path, &revision_id)
            .await
            .map_err(|e| get_revision_error(e, rid))?;
        let etag = quoted_etag(record.revision.as_str());
        if format.as_deref() == Some("meta") {
            return Ok(contract::FilesGetRevisionResponse {
                r#return: contract::RevisionPayload::Json(revision_dto(&record, "unknown")),
                etag: Some(etag),
                media_type: None,
                content_disposition: None,
            });
        }
        let bytes = self
            .state
            .vault
            .revision_bytes(&record)
            .await
            .map_err(|e| get_revision_error(e, rid))?;
        let media = media_type_or_default(record.content.as_ref());
        let (media_type, content_disposition) = raw_headers(&media, path.file_name());
        Ok(contract::FilesGetRevisionResponse {
            r#return: contract::RevisionPayload::OctetStream(bytes),
            etag: Some(etag),
            media_type,
            content_disposition,
        })
    }

    async fn restore(
        &self,
        if_match: Option<String>,
        operation_id: Option<String>,
        body: contract::RestoreBody,
    ) -> Result<contract::FilesRestoreResponse, contract::FilesRestoreError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&body.path).map_err(|e| restore_error(e, rid))?;
        let if_match = required_if_match(if_match.as_deref()).map_err(|e| restore_error(e, rid))?;
        let op_id = parse_operation_id(operation_id.as_ref()).map_err(|e| restore_error(e, rid))?;
        let source = HexId::from_hex(&body.revision).map_err(|e| restore_error(e, rid))?;
        let outcome = self
            .state
            .vault
            .restore_file(
                ctx.auth.principal(),
                &path,
                &source,
                Precondition::IfRevision(if_match),
                ctx.auth.principal().actor.clone(),
                op_id,
            )
            .await
            .map_err(|e| restore_error(e, rid))?;
        Ok(contract::FilesRestoreResponse {
            r#return: mutation_result(&outcome),
            etag: Some(quoted_etag(outcome.revision)),
        })
    }

    async fn search(
        &self,
        body: contract::SearchQuery,
    ) -> Result<contract::SearchPage, contract::FilesSearchError> {
        let contract::SearchQuery {
            q,
            mode,
            prefix,
            tags,
            status,
            r#type,
            trust,
            stale,
            scan_limit,
            cursor,
        } = body;
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let mode = match mode.as_deref().unwrap_or("literal") {
            "literal" => brains_application::search::SearchMode::Literal,
            "fts" => brains_application::search::SearchMode::Fts,
            "hybrid" => brains_application::search::SearchMode::Hybrid,
            other => {
                return Err(search_error(
                    DomainError::InvalidArgument(format!("unknown search mode `{other}`")),
                    rid,
                ));
            }
        };
        let scan_limit = scan_limit
            .map(|v| {
                usize::try_from(v).map_err(|_| {
                    search_error(
                        DomainError::InvalidArgument("scan_limit must be positive".into()),
                        rid,
                    )
                })
            })
            .transpose()?
            .unwrap_or(self.state.limits.scan_window_max);
        let query = brains_application::search::SearchQuery {
            q: q.clone(),
            mode,
            prefix,
            tags: tags.unwrap_or_default(),
            status,
            type_: r#type,
            trust,
            stale,
            cursor,
            scan_limit,
        };
        let result = self
            .state
            .vault
            .search(ctx.auth.principal(), query)
            .await
            .map_err(|e| search_error(e, rid))?;
        Ok(contract::SearchPage {
            items: result
                .items
                .into_iter()
                .map(|hit| contract::SearchHit {
                    path: hit.path,
                    indexed_revision: hit.indexed_revision,
                    snippet: hit.snippet,
                    retrieved_by: hit
                        .retrieved_by
                        .into_iter()
                        .map(|retrieval| contract::Retrieval {
                            retriever: retrieval.retriever,
                            rank: retrieval.rank as u64,
                        })
                        .collect(),
                })
                .collect(),
            next_cursor: result.next_cursor,
            complete: result.complete,
            scanned: result.scanned as i64,
            unavailable: result.unavailable,
            index_state: if result.building {
                "building"
            } else {
                "available"
            }
            .into(),
        })
    }

    async fn links(
        &self,
        path: String,
        direction: Option<String>,
        cursor: Option<String>,
    ) -> Result<contract::LinkPage, contract::FilesLinksError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| links_error(e, rid))?;
        let backlinks = match direction.as_deref().unwrap_or("outbound") {
            "outbound" => false,
            "backlinks" => true,
            other => {
                return Err(links_error(
                    DomainError::InvalidArgument(format!("unknown direction `{other}`")),
                    rid,
                ));
            }
        };
        let page = self
            .state
            .vault
            .links(
                ctx.auth.principal(),
                &path,
                backlinks,
                cursor.as_deref(),
                50,
            )
            .await
            .map_err(|e| links_error(e, rid))?;
        Ok(contract::LinkPage {
            items: page
                .rows
                .into_iter()
                .map(|row| contract::LinkItem {
                    source: row.source,
                    target: row.target,
                    kind: row.kind,
                    source_revision: Some(row.source_revision),
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    async fn copy(
        &self,
        body: contract::TransferBody,
    ) -> Result<contract::TransferResult, contract::FilesCopyError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        match self.transfer(body, TransferKind::Copy).await {
            TransferOutcome::Done(result) => Ok(result),
            TransferOutcome::Partial(result) => {
                Err(contract::FilesCopyError::TransferConflict(result.into()))
            }
            TransferOutcome::Failed(e) => Err(copy_error(e, rid)),
        }
    }

    async fn r#move(
        &self,
        body: contract::TransferBody,
    ) -> Result<contract::TransferResult, contract::FilesMoveError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        match self.transfer(body, TransferKind::Move).await {
            TransferOutcome::Done(result) => Ok(result),
            TransferOutcome::Partial(result) => {
                Err(contract::FilesMoveError::TransferConflict(result.into()))
            }
            TransferOutcome::Failed(e) => Err(move_error(e, rid)),
        }
    }

    async fn diff(
        &self,
        path: String,
        from: String,
        to: String,
    ) -> Result<contract::DiffResult, contract::FilesDiffError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let path = VaultPath::new(&path).map_err(|e| diff_error(e, rid))?;
        let from = HexId::from_hex(&from).map_err(|e| diff_error(e, rid))?;
        let to = HexId::from_hex(&to).map_err(|e| diff_error(e, rid))?;
        let diff = self
            .state
            .vault
            .diff_revisions(ctx.auth.principal(), &path, &from, &to)
            .await
            .map_err(|e| diff_error(e, rid))?;
        Ok(contract::DiffResult { diff })
    }

    async fn reindex(
        &self,
        body: contract::ReindexRequest,
    ) -> Result<contract::ReindexPage, contract::FilesReindexError> {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let limit = match body.limit {
            None => self.state.limits.reindex_page_max,
            Some(raw) => {
                let max = self.state.limits.reindex_page_max as i64;
                if !(1..=max).contains(&raw) {
                    return Err(reindex_error(
                        DomainError::InvalidArgument(format!("limit must be between 1 and {max}")),
                        rid,
                    ));
                }
                raw as usize
            }
        };
        let page = self
            .state
            .vault
            .reindex_page(ctx.auth.principal(), body.cursor.as_deref(), limit)
            .await
            .map_err(|e| reindex_error(e, rid))?;
        Ok(contract::ReindexPage {
            processed: page.processed as i64,
            failed: page
                .failed
                .into_iter()
                .map(|f| contract::ReindexFailure {
                    path: f.path,
                    code: f.code,
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    async fn validate(&self) -> Result<(), contract::FilesValidateError> {
        let ctx = current();
        let err = DomainError::CapabilityUnsupported("server_side_validate");
        Err(contract::FilesValidateError::NotImplemented(exception!(
            NotImplemented,
            &err,
            ctx.request_id.as_str()
        )))
    }
}
