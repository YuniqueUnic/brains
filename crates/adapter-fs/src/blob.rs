//! Immutable blob storage: create-only, digest-verified (design §8.2 step 3).

use sha2::{Digest, Sha256};

use brains_domain::{DomainError, DomainResult};
use brains_ports::BlobStore;

use crate::FsStore;

pub struct FsBlobStore {
    store: FsStore,
}

impl FsBlobStore {
    pub fn new(store: FsStore) -> Self {
        Self { store }
    }

    /// Parse `vaults/<vault>/blobs/<revision>/<file_name>` into a path under
    /// this store. Internal keys, never URLs: no percent-decoding, no `..`.
    fn blob_path(&self, key: &str) -> DomainResult<std::path::PathBuf> {
        let prefix = format!("vaults/{}/blobs/", self.store.vault());
        let rest = key
            .strip_prefix(&prefix)
            .ok_or_else(|| DomainError::Internal(format!("blob key outside vault: {key}")))?;
        let (revision, file_name) = rest
            .split_once('/')
            .ok_or_else(|| DomainError::Internal(format!("malformed blob key: {key}")))?;
        brains_domain::revision::validate_blob_key(key, file_name)?;
        if file_name.contains('/') {
            return Err(DomainError::Internal(format!("malformed blob key: {key}")));
        }
        crate::paths::join(&self.store.blobs_dir().join(revision), file_name)
    }
}

impl BlobStore for FsBlobStore {
    /// Verify the digest and stage bytes to a temp file, then
    /// hard-link into place — `hard_link` fails on an existing target, which
    /// gives create-only semantics without a rename race.
    async fn create(&self, key: &str, content: &[u8], sha256_hex: &str) -> DomainResult<()> {
        let target = self.blob_path(key)?;
        if hex::encode(Sha256::digest(content)) != sha256_hex {
            return Err(DomainError::ChecksumMismatch {
                path: key.to_owned(),
            });
        }
        crate::create_immutable(&target, content)
    }

    async fn get(&self, key: &str) -> DomainResult<Vec<u8>> {
        let path = self.blob_path(key)?;
        std::fs::read(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound {
                    path: key.to_owned(),
                }
            } else {
                DomainError::StorageUnavailable(format!("blob read: {e}"))
            }
        })
    }
}
