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
        Ok(self.store.blobs_dir().join(revision).join(file_name))
    }
}

impl BlobStore for FsBlobStore {
    /// Stage the bytes to a temp file while hashing, verify the digest, then
    /// hard-link into place — `hard_link` fails on an existing target, which
    /// gives create-only semantics without a rename race.
    async fn create(&self, key: &str, content: &[u8], sha256_hex: &str) -> DomainResult<()> {
        let target = self.blob_path(key)?;
        let dir = target
            .parent()
            .ok_or_else(|| DomainError::Internal(format!("blob key without parent: {key}")))?;
        std::fs::create_dir_all(dir)
            .map_err(|e| DomainError::StorageUnavailable(format!("blob dir: {e}")))?;
        let temp = dir.join(format!(".staging-{}", uuid::Uuid::new_v4().simple()));
        let write = || -> DomainResult<()> {
            let mut hasher = Sha256::new();
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| DomainError::StorageUnavailable(format!("blob staging: {e}")))?;
            use std::io::Write;
            // Hash in chunks to keep peak copies bounded for large attachments.
            for chunk in content.chunks(1024 * 1024) {
                hasher.update(chunk);
                file.write_all(chunk)
                    .and_then(|()| file.flush())
                    .map_err(|e| DomainError::StorageUnavailable(format!("blob write: {e}")))?;
            }
            file.sync_all()
                .map_err(|e| DomainError::StorageUnavailable(format!("blob fsync: {e}")))?;
            let actual = hex::encode(hasher.finalize());
            if actual != sha256_hex {
                let _ = std::fs::remove_file(&temp);
                return Err(DomainError::ChecksumMismatch {
                    path: key.to_owned(),
                });
            }
            Ok(())
        };
        write()?;
        if let Err(e) = std::fs::hard_link(&temp, &target) {
            let _ = std::fs::remove_file(&temp);
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                return Err(DomainError::StorageUnavailable(format!(
                    "immutable blob collision: {key}"
                )));
            }
            return Err(DomainError::StorageUnavailable(format!("blob commit: {e}")));
        }
        let _ = std::fs::remove_file(&temp);
        crate::fsync_dir(dir)?;
        Ok(())
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
