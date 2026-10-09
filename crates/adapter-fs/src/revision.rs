//! Immutable revision records: create-only JSON files keyed by revision ID.

use brains_domain::{DomainError, DomainResult, HexId, Revision};
use brains_ports::RevisionStore;

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
        let bytes = serde_json::to_vec(revision)
            .map_err(|e| DomainError::Internal(format!("revision serialization: {e}")))?;
        crate::create_immutable(&path, &bytes)
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
