//! Embedded adapter: the same application services used by the native server,
//! driven synchronously without HTTP, a listener, or an authentication store.
use crate::{
    ClientResult, Library, Precondition, TransferKind,
    archive::{ArchiveEndpoint, ArchiveTarget},
};
use brains_api_types::{
    DiffResult, Entry, ErrorDetail, FilePage, HistoryPage, LinkItem, LinkPage, MutationResult,
    PatchBody, PatchResult, ReindexFailure, ReindexPage, RevisionDto, SearchHit, SearchPage,
    Session, TransferBody, TransferResult,
};
use brains_application::{
    MutationOutcome,
    commit::CommitInput,
    patch::{PatchEdit, PatchInput},
    transfer::TransferInput,
};
use brains_domain::{Capabilities, DomainError, Head, HexId, Principal, Revision, VaultPath};
use futures::executor::block_on;
use std::path::Path;

pub struct LocalClient {
    vault: brains_local::LocalVault,
    principal: Principal,
}
impl LocalClient {
    pub fn open(data_dir: &Path, vault: &str) -> ClientResult<Self> {
        Ok(Self {
            vault: brains_local::open(data_dir, vault)?,
            principal: brains_local::owner(),
        })
    }
    pub fn create(data_dir: &Path, vault: &str) -> ClientResult<Self> {
        Ok(Self {
            vault: brains_local::create(data_dir, vault)?,
            principal: brains_local::owner(),
        })
    }
}
fn operation(raw: Option<&str>) -> ClientResult<HexId> {
    Ok(match raw {
        Some(raw) => HexId::from_hex(raw)?,
        None => HexId::generate(),
    })
}
fn precondition(value: Precondition) -> ClientResult<brains_application::Precondition> {
    Ok(match value {
        Precondition::Create => brains_application::Precondition::Create,
        Precondition::IfRevision(rev) => {
            brains_application::Precondition::IfRevision(HexId::from_hex(&rev)?)
        }
    })
}
fn mutation(value: MutationOutcome) -> MutationResult {
    MutationResult {
        entry: Entry {
            path: value.path,
            revision: value.revision,
            generation: value.generation,
            state: value.state.as_str().into(),
            updated_at: Some(value.updated_at),
            size: Some(value.size),
            sha256: (!value.sha256.is_empty()).then_some(value.sha256),
            media_type: Some(value.media_type),
            kind: Some(value.kind),
            title: None,
        },
        index_state: value.index_state.as_str().into(),
        operation_id: value.operation_id,
    }
}
fn limit(value: usize, max: usize) -> ClientResult<()> {
    if value == 0 || value > max {
        return Err(
            DomainError::InvalidArgument(format!("limit must be between 1 and {max}")).into(),
        );
    }
    Ok(())
}
impl Library for LocalClient {
    fn capabilities(&self) -> ClientResult<Capabilities> {
        Ok(Capabilities::local(
            brains_build_info::VERSION,
            &self.vault.vault,
            self.vault.limits.max_text_bytes,
            self.vault.limits.max_attachment_bytes,
            brains_ports::IndexStore::full_text_search(&self.vault.index),
        ))
    }
    fn session(&self) -> ClientResult<Session> {
        Ok(Session {
            actor: self.principal.actor.clone(),
            scopes: self
                .principal
                .scopes
                .iter()
                .map(|s| s.as_string())
                .collect(),
            expires_at: None,
            csrf_token: None,
        })
    }
    fn list_files(&self, query: &brains_application::listing::ListQuery) -> ClientResult<FilePage> {
        let page = block_on(self.vault.list_files(&self.principal, query.clone()))?;
        Ok(FilePage {
            items: page.items.iter().map(Entry::from_info).collect(),
            next_cursor: page.next_cursor,
            source: page.source.to_string(),
            consistency: "per_file".into(),
        })
    }
    fn get_meta(&self, path: &str) -> ClientResult<Entry> {
        let info = block_on(
            self.vault
                .read_entry(&self.principal, &VaultPath::new(path)?),
        )?;
        Ok(Entry::from_info(&info))
    }
    fn get_raw(&self, path: &str) -> ClientResult<(Vec<u8>, String)> {
        let file = block_on(
            self.vault
                .read_file(&self.principal, &VaultPath::new(path)?),
        )?;
        Ok((file.bytes, format!("\"{}\"", file.head.revision)))
    }
    fn put_content(
        &self,
        path: &str,
        body: &[u8],
        media_type: &str,
        executable: Option<bool>,
        expected: Precondition,
        operation_id: Option<&str>,
    ) -> ClientResult<MutationResult> {
        let path = VaultPath::new(path)?;
        let input = CommitInput {
            path: &path,
            content: body,
            media_type: Some(media_type),
            actor: self.principal.actor.clone(),
            operation_id: operation(operation_id)?,
            precondition: precondition(expected)?,
            executable,
            copied_from: None,
        };
        Ok(mutation(block_on(
            self.vault.put_file(&self.principal, input),
        )?))
    }
    fn delete(
        &self,
        path: &str,
        if_revision: &str,
        operation_id: Option<&str>,
    ) -> ClientResult<MutationResult> {
        Ok(mutation(block_on(self.vault.trash_file(
            &self.principal,
            &VaultPath::new(path)?,
            precondition(Precondition::IfRevision(if_revision.into()))?,
            self.principal.actor.clone(),
            operation(operation_id)?,
        ))?))
    }
    fn history(
        &self,
        path: &str,
        cursor: Option<&str>,
        page_limit: usize,
    ) -> ClientResult<HistoryPage> {
        limit(page_limit, self.vault.limits.history_page_max)?;
        let page = block_on(self.vault.history_page(
            &self.principal,
            &VaultPath::new(path)?,
            cursor,
            page_limit,
        ))?;
        Ok(HistoryPage {
            items: page
                .items
                .iter()
                .map(|r| RevisionDto::from_revision(r, "reachable"))
                .collect(),
            anchor_revision: page.anchor_revision,
            next_cursor: page.next_cursor,
        })
    }
    fn revision_meta(&self, path: &str, revision: &str) -> ClientResult<RevisionDto> {
        let revision = block_on(self.vault.read_revision(
            &self.principal,
            &VaultPath::new(path)?,
            &HexId::from_hex(revision)?,
        ))?;
        Ok(RevisionDto::from_revision(&revision, "unknown"))
    }
    fn revision_raw(&self, path: &str, revision: &str) -> ClientResult<Vec<u8>> {
        let revision = block_on(self.vault.read_revision(
            &self.principal,
            &VaultPath::new(path)?,
            &HexId::from_hex(revision)?,
        ))?;
        Ok(block_on(self.vault.revision_bytes(&revision))?)
    }
    fn restore(
        &self,
        path: &str,
        revision: &str,
        if_current: &str,
        operation_id: Option<&str>,
    ) -> ClientResult<MutationResult> {
        Ok(mutation(block_on(self.vault.restore_file(
            &self.principal,
            &VaultPath::new(path)?,
            &HexId::from_hex(revision)?,
            precondition(Precondition::IfRevision(if_current.into()))?,
            self.principal.actor.clone(),
            operation(operation_id)?,
        ))?))
    }
    fn search(&self, query: &brains_application::search::SearchQuery) -> ClientResult<SearchPage> {
        let result = block_on(self.vault.search(&self.principal, query.clone()))?;
        Ok(SearchPage {
            items: result
                .items
                .into_iter()
                .map(|h| SearchHit {
                    path: h.path,
                    indexed_revision: h.indexed_revision,
                    snippet: h.snippet,
                })
                .collect(),
            next_cursor: result.next_cursor,
            complete: result.complete,
            scanned: result.scanned as i64,
            index_state: if result.building {
                "building"
            } else {
                "available"
            }
            .into(),
        })
    }
    fn links(&self, path: &str, backlinks: bool, cursor: Option<&str>) -> ClientResult<LinkPage> {
        let page = block_on(self.vault.links(
            &self.principal,
            &VaultPath::new(path)?,
            backlinks,
            cursor,
            50,
        ))?;
        Ok(LinkPage {
            items: page
                .rows
                .into_iter()
                .map(|r| LinkItem {
                    source: r.source,
                    target: r.target,
                    kind: r.kind,
                    source_revision: Some(r.source_revision),
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }
    fn reindex(&self, cursor: Option<&str>, page_limit: usize) -> ClientResult<ReindexPage> {
        limit(page_limit, self.vault.limits.reindex_page_max)?;
        let page = block_on(self.vault.reindex_page(&self.principal, cursor, page_limit))?;
        Ok(ReindexPage {
            processed: page.processed as i64,
            failed: page
                .failed
                .into_iter()
                .map(|f| ReindexFailure {
                    path: f.path,
                    code: f.code,
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }
    fn patch(
        &self,
        path: &str,
        if_revision: &str,
        body: PatchBody,
        operation_id: Option<&str>,
    ) -> ClientResult<PatchResult> {
        let edits = body
            .edits
            .into_iter()
            .map(|e| {
                let expected_matches = usize::try_from(e.expected_matches).map_err(|_| {
                    DomainError::InvalidArgument("expected_matches must be nonnegative".into())
                })?;
                Ok(PatchEdit {
                    old_text: e.old_text,
                    new_text: e.new_text,
                    expected_matches,
                })
            })
            .collect::<Result<Vec<_>, DomainError>>()?;
        let result = block_on(self.vault.patch_file(
            &self.principal,
            &VaultPath::new(path)?,
            precondition(Precondition::IfRevision(if_revision.into()))?,
            self.principal.actor.clone(),
            operation(operation_id)?,
            PatchInput {
                edits,
                dry_run: body.dry_run,
            },
        ))?;
        Ok(PatchResult {
            diff: result.diff,
            base_revision: result.base_revision,
            committed: result.committed.map(mutation),
        })
    }
    fn transfer(&self, kind: TransferKind, body: TransferBody) -> ClientResult<TransferResult> {
        let input = TransferInput {
            source: VaultPath::new(&body.source)?,
            target: VaultPath::new(&body.target)?,
            expected_source_revision: HexId::from_hex(&body.expected_source_revision)?,
            actor: self.principal.actor.clone(),
            operation_id: HexId::from_hex(&body.operation_id)?,
        };
        let result = block_on(async {
            match kind {
                TransferKind::Copy => self.vault.copy_file(&self.principal, input).await,
                TransferKind::Move => self.vault.move_file(&self.principal, input).await,
            }
        })?;
        Ok(TransferResult {
            copied: result.copied,
            source_trashed: result.source_trashed,
            partial: result.partial,
            operation_id: result.operation_id,
            target_revision: result.target_revision,
            source_revision: result.source_revision,
            error: result.error.map(|e| ErrorDetail {
                code: e.code().into(),
                message: e.to_string(),
                request_id: String::new(),
                path: e.path().map(str::to_owned),
                current_revision: e.current_revision().map(str::to_owned),
                details: None,
            }),
        })
    }
    fn diff(&self, path: &str, from: &str, to: &str) -> ClientResult<DiffResult> {
        Ok(DiffResult {
            diff: block_on(self.vault.diff_revisions(
                &self.principal,
                &VaultPath::new(path)?,
                &HexId::from_hex(from)?,
                &HexId::from_hex(to)?,
            ))?,
        })
    }
}
impl ArchiveEndpoint for LocalClient {
    fn archive_target(&self) -> ClientResult<ArchiveTarget> {
        Ok(ArchiveTarget {
            vault: self.vault.vault.clone(),
            max_blob_bytes: self.vault.limits.max_attachment_bytes,
        })
    }
    fn archive_revision(&self, path: &str, revision: &str) -> ClientResult<Revision> {
        Ok(block_on(self.vault.archive_revision(
            &self.principal,
            &VaultPath::new(path)?,
            &HexId::from_hex(revision)?,
        ))?)
    }
    fn store_revision(&self, revision: &Revision) -> ClientResult<Revision> {
        Ok(block_on(
            self.vault.import_revision(&self.principal, revision),
        )?)
    }
    fn store_blob(&self, revision: &Revision, bytes: &[u8]) -> ClientResult<()> {
        Ok(block_on(self.vault.import_blob(
            &self.principal,
            &VaultPath::new(&revision.path)?,
            &revision.revision,
            bytes,
        ))?)
    }
    fn publish_head(&self, revision: &Revision) -> ClientResult<Head> {
        Ok(block_on(self.vault.import_head(
            &self.principal,
            &VaultPath::new(&revision.path)?,
            &revision.revision,
        ))?)
    }
    fn rebuild_index(&self) -> ClientResult<()> {
        crate::verify::rebuild_index(self)
    }
}
