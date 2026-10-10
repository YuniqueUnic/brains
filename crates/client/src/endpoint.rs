//! Shared conditional file operations for transfer, export and sync.
use crate::{ClientError, ClientResult, Library, Precondition};
use brains_api_types::{ContentRefDto, Entry, MutationResult, RevisionDto};
use sha2::{Digest, Sha256};

/// Operations required by the sync engine. Implementations must retain the
/// server's revision preconditions; writes are never unconditional.
pub trait FileEndpoint {
    fn current_files(&self) -> ClientResult<Vec<Entry>>;
    fn revision_meta(&self, path: &str, revision: &str) -> ClientResult<RevisionDto>;
    fn revision_bytes(&self, path: &str, revision: &str) -> ClientResult<Vec<u8>>;
    fn put(
        &self,
        path: &str,
        bytes: &[u8],
        media_type: &str,
        executable: bool,
        precondition: Precondition,
    ) -> ClientResult<MutationResult>;
    fn trash(&self, path: &str, revision: &str) -> ClientResult<MutationResult>;
    fn restore(&self, path: &str, revision: &str, if_current: &str)
    -> ClientResult<MutationResult>;
}

impl<T: Library + ?Sized> FileEndpoint for T {
    fn current_files(&self) -> ClientResult<Vec<Entry>> {
        let mut files = Vec::new();
        let mut paths = std::collections::BTreeSet::new();
        let mut cursors = std::collections::BTreeSet::new();
        let mut cursor: Option<String> = None;
        loop {
            let query = brains_application::listing::ListQuery {
                source: brains_application::listing::ListSource::Store,
                include_deleted: true,
                limit: 100,
                cursor: cursor.clone(),
                ..Default::default()
            };
            let page = self.list_files(&query)?;
            if page.source != "store" {
                return Err(invalid("server did not return authoritative storage"));
            }
            for entry in page.items {
                brains_domain::VaultPath::new(&entry.path).map_err(|e| invalid(&e.to_string()))?;
                brains_domain::HexId::from_hex(&entry.revision)
                    .map_err(|e| invalid(&e.to_string()))?;
                if !matches!(entry.state.as_str(), "live" | "trashed")
                    || entry.generation < 1
                    || !paths.insert(entry.path.clone())
                {
                    return Err(invalid("invalid or duplicate authoritative entry"));
                }
                files.push(entry);
            }
            match page.next_cursor {
                Some(next) if cursors.insert(next.clone()) => cursor = Some(next),
                Some(_) => return Err(invalid("repeated listing cursor")),
                None => {
                    files.sort_by(|a, b| a.path.cmp(&b.path));
                    return Ok(files);
                }
            }
        }
    }

    fn revision_meta(&self, path: &str, revision: &str) -> ClientResult<RevisionDto> {
        Library::revision_meta(self, path, revision)
    }

    fn revision_bytes(&self, path: &str, revision: &str) -> ClientResult<Vec<u8>> {
        Library::revision_raw(self, path, revision)
    }

    fn put(
        &self,
        path: &str,
        bytes: &[u8],
        media_type: &str,
        executable: bool,
        precondition: Precondition,
    ) -> ClientResult<MutationResult> {
        self.put_content(
            path,
            bytes,
            media_type,
            Some(executable),
            precondition,
            None,
        )
    }

    fn trash(&self, path: &str, revision: &str) -> ClientResult<MutationResult> {
        self.delete(path, revision, None)
    }

    fn restore(
        &self,
        path: &str,
        revision: &str,
        if_current: &str,
    ) -> ClientResult<MutationResult> {
        Library::restore(self, path, revision, if_current, None)
    }
}

/// Download the immutable revision and validate the metadata/content boundary.
pub fn read_verified(
    endpoint: &dyn FileEndpoint,
    path: &str,
    revision: &str,
) -> ClientResult<(Vec<u8>, ContentRefDto)> {
    let meta = endpoint.revision_meta(path, revision)?;
    if meta.path != path || meta.revision != revision {
        return Err(invalid(
            "revision identity does not match the requested file",
        ));
    }
    let content = meta
        .content
        .ok_or_else(|| invalid("live revision has no content"))?;
    let bytes = endpoint.revision_bytes(path, revision)?;
    if bytes.len() as i64 != content.size || hex::encode(Sha256::digest(&bytes)) != content.sha256 {
        return Err(invalid(&format!(
            "revision checksum/size mismatch for {path} @ {revision}"
        )));
    }
    Ok((bytes, content))
}

pub(crate) fn invalid(message: &str) -> ClientError {
    ClientError {
        status: 422,
        code: Some("INVALID_FORMAT".into()),
        message: message.to_owned(),
        current_revision: None,
    }
}
