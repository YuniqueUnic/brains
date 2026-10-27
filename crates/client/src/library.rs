//! Operations used by CLI commands, independent of the connection mode.
use crate::{ClientResult, Precondition, TransferKind};
use brains_api_types::{
    DiffResult, Entry, FilePage, HistoryPage, LinkPage, MutationResult, PatchBody, PatchResult,
    ReindexPage, SearchPage, Session, TransferBody, TransferResult,
};
use brains_domain::Capabilities;

pub trait Library: crate::archive::ArchiveEndpoint {
    fn convert_document(
        &self,
        _path: &str,
        _to: Option<&str>,
    ) -> ClientResult<crate::convert::ConvertReport> {
        Err(crate::ClientError {
            status: 501,
            code: Some("CAPABILITY_UNSUPPORTED".into()),
            message: "remote conversion is not available".into(),
            current_revision: None,
        })
    }
    fn import_public_url(
        &self,
        _url: &str,
        _path: &str,
    ) -> ClientResult<brains_sdk::UrlImportResult> {
        Err(crate::ClientError {
            status: 501,
            code: Some("CAPABILITY_UNSUPPORTED".into()),
            message: "remote URL import is not available".into(),
            current_revision: None,
        })
    }
    fn resource_status(&self) -> ClientResult<brains_sdk::ResourceReport> {
        Err(crate::ClientError {
            status: 501,
            code: Some("CAPABILITY_UNSUPPORTED".into()),
            message: "resource policy is not available".into(),
            current_revision: None,
        })
    }
    fn capabilities(&self) -> ClientResult<Capabilities>;
    /// Choose a maintenance batch size within the server's advertised limit.
    fn reindex_page_size(&self, requested: usize) -> ClientResult<usize> {
        Ok(self
            .capabilities()?
            .limits
            .reindex_page_max
            .map_or(requested, |maximum| requested.min(maximum)))
    }
    fn session(&self) -> ClientResult<Session>;
    fn list_files(&self, query: &brains_application::listing::ListQuery) -> ClientResult<FilePage>;
    fn get_meta(&self, path: &str) -> ClientResult<Entry>;
    fn get_raw(&self, path: &str) -> ClientResult<(Vec<u8>, String)>;
    fn put_content(
        &self,
        path: &str,
        body: &[u8],
        media_type: &str,
        executable: Option<bool>,
        precondition: Precondition,
        operation_id: Option<&str>,
    ) -> ClientResult<MutationResult>;
    fn delete(
        &self,
        path: &str,
        if_revision: &str,
        operation_id: Option<&str>,
    ) -> ClientResult<MutationResult>;
    fn history(&self, path: &str, cursor: Option<&str>, limit: usize) -> ClientResult<HistoryPage>;
    fn revision_meta(
        &self,
        path: &str,
        revision: &str,
    ) -> ClientResult<brains_api_types::RevisionDto>;
    fn revision_raw(&self, path: &str, revision: &str) -> ClientResult<Vec<u8>>;
    fn restore(
        &self,
        path: &str,
        revision: &str,
        if_current: &str,
        operation_id: Option<&str>,
    ) -> ClientResult<MutationResult>;
    fn search(&self, query: &brains_application::search::SearchQuery) -> ClientResult<SearchPage>;
    fn links(&self, path: &str, backlinks: bool, cursor: Option<&str>) -> ClientResult<LinkPage>;
    fn reindex(&self, cursor: Option<&str>, limit: usize) -> ClientResult<ReindexPage>;
    fn patch(
        &self,
        path: &str,
        if_revision: &str,
        body: PatchBody,
        operation_id: Option<&str>,
    ) -> ClientResult<PatchResult>;
    fn transfer(&self, kind: TransferKind, body: TransferBody) -> ClientResult<TransferResult>;
    fn diff(&self, path: &str, from: &str, to: &str) -> ClientResult<DiffResult>;
}
