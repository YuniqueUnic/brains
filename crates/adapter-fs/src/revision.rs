//! Immutable revision records: create-only JSON files keyed by revision ID.

use brainstore_domain::{DomainError, DomainResult, HexId, Revision};
use brainstore_ports::RevisionStore;

use crate::{FsStore, read_json_file};

pub struct FsRevisionStore {
    store: FsStore,
}

impl FsRevisionStore {
    pub fn new(store: FsStore) -> Self {
        Self { store }
    }
}

impl RevisionStore for FsRevisionStore {
    async fn create(&self, revision: &Revision) -> DomainResult<()> {
        revision.validate()?;
        let path = self.store.revision_file(&revision.revision);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DomainError::StorageUnavailable(format!("revisions dir: {e}")))?;
        }
        let bytes = serde_json::to_vec(revision)
            .map_err(|e| DomainError::Internal(format!("revision serialization: {e}")))?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    DomainError::Internal("revision id collision".into())
                } else {
                    DomainError::StorageUnavailable(format!("revision write: {e}"))
                }
            })?;
        std::io::Write::write_all(&mut file, &bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| DomainError::StorageUnavailable(format!("revision write: {e}")))?;
        crate::fsync_dir(path.parent().unwrap())
    }

    async fn get(&self, revision: &HexId) -> DomainResult<Option<Revision>> {
        let path = self.store.revision_file(revision);
        if !path.exists() {
            return Ok(None);
        }
        let value = read_json_file(&path)?;
        let parsed: Revision = serde_json::from_value(value).map_err(|e| {
            DomainError::StorageUnavailable(format!("corrupt revision record: {e}"))
        })?;
        if parsed.revision != *revision {
            return Err(DomainError::StorageUnavailable(
                "revision identity mismatch".into(),
            ));
        }
        parsed.validate()?;
        Ok(Some(parsed))
    }
}
