//! Filesystem content store (design §5.3, §7.1, §9.3). The on-disk layout is
//! exactly the raw-backup layout, so a copy of this tree *is* a raw backup:
//!
//! ```text
//! <base>/vaults/<vault>/
//! ├── format.json
//! ├── heads/<path>.json
//! ├── revisions/<revision>.json
//! ├── blobs/<revision>/<file_name>
//! └── locks/<sha256(path)>.lock
//! ```
//!
//! Every writer must go through [`FsStore`]: HEAD updates take a cross-process
//! file lock, verify the expected token, then atomically replace the HEAD
//! file (temp write → fsync → rename → parent fsync). External editors never
//! touch these files directly; they edit a checkout and push via the API.

pub mod blob;
pub mod head;
mod paths;
pub mod revision;

use std::io;
use std::path::{Path, PathBuf};

#[cfg(not(windows))]
use sha2::{Digest, Sha256};

use brains_domain::{DomainError, DomainResult};

pub use blob::FsBlobStore;
pub use head::FsHeadStore;
pub use revision::FsRevisionStore;

/// Handle on one vault's durable store. Cheap to clone (path holder).
#[derive(Clone, Debug)]
pub struct FsStore {
    vault_dir: PathBuf,
    vault: String,
    // Every adapter clone keeps the process ownership alive.
    _ownership: std::sync::Arc<std::fs::File>,
}

pub fn validate_vault_name(vault: &str) -> DomainResult<()> {
    let ok = !vault.is_empty()
        && vault.len() <= 32
        && vault
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(DomainError::InvalidArgument(
            "vault name must match [a-z0-9_-]{1,32}".into(),
        ))
    }
}

impl FsStore {
    /// Open an existing store, requiring a matching format marker.
    pub fn open(base: &Path, vault: &str) -> DomainResult<Self> {
        validate_vault_name(vault)?;
        let store = Self {
            vault_dir: base.join("vaults").join(vault),
            vault: vault.to_owned(),
            _ownership: Self::acquire(base, false)?,
        };
        let format = read_json_file(&store.vault_dir.join("format.json"))
            .map_err(|e| DomainError::StorageUnavailable(format!("store open failed: {e}")))?;
        let ok = format.get("format").and_then(|v| v.as_str()) == Some("brains")
            && format.get("format_version").and_then(|v| v.as_i64()) == Some(1)
            && format.get("vault").and_then(|v| v.as_str()) == Some(vault);
        if !ok {
            return Err(DomainError::StorageUnavailable(
                "unsupported or mismatched store format".into(),
            ));
        }
        Ok(store)
    }

    /// Create a fresh store (create-only on format.json).
    pub fn create(base: &Path, vault: &str) -> DomainResult<Self> {
        validate_vault_name(vault)?;
        let store = Self {
            vault_dir: base.join("vaults").join(vault),
            vault: vault.to_owned(),
            _ownership: Self::acquire(base, true)?,
        };
        for dir in [
            store.heads_dir(),
            store.revisions_dir(),
            store.blobs_dir(),
            store.locks_dir(),
        ] {
            std::fs::create_dir_all(&dir)
                .map_err(|e| DomainError::StorageUnavailable(format!("{}: {e}", dir.display())))?;
        }
        let format = serde_json::json!({
            "format": "brains",
            "format_version": 1,
            "vault": vault,
        });
        let path = store.vault_dir.join("format.json");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    DomainError::StorageUnavailable("store already exists".into())
                } else {
                    DomainError::StorageUnavailable(format!("format.json: {e}"))
                }
            })?;
        serde_json::to_writer_pretty(&mut file, &format)
            .and_then(|()| file.sync_all().map_err(serde_json::Error::io))
            .map_err(|e| DomainError::StorageUnavailable(format!("format.json: {e}")))?;
        Ok(store)
    }

    fn acquire(base: &Path, create: bool) -> DomainResult<std::sync::Arc<std::fs::File>> {
        if create {
            std::fs::create_dir_all(base)
                .map_err(|e| DomainError::StorageUnavailable(e.to_string()))?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(base.join(".brains.lock"))
            .map_err(|e| {
                DomainError::StorageUnavailable(format!("open {}: {e}", base.display()))
            })?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|e| DomainError::StorageUnavailable(
            if e.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
                format!("data directory {} is in use; close its local process, or use a remote profile to access the running server", base.display())
            } else { format!("lock {}: {e}", base.display()) }
        ))?;
        Ok(std::sync::Arc::new(file))
    }

    pub fn vault(&self) -> &str {
        &self.vault
    }

    pub fn vault_dir(&self) -> &Path {
        &self.vault_dir
    }

    pub fn heads_dir(&self) -> PathBuf {
        self.vault_dir.join("heads")
    }

    pub fn revisions_dir(&self) -> PathBuf {
        self.vault_dir.join("revisions")
    }

    pub fn blobs_dir(&self) -> PathBuf {
        self.vault_dir.join("blobs")
    }

    pub fn locks_dir(&self) -> PathBuf {
        self.vault_dir.join("locks")
    }

    /// Storage file for a logical path: `heads/<path>.json`.
    pub fn head_file(&self, path: &brains_domain::VaultPath) -> DomainResult<PathBuf> {
        paths::validate_name(path.as_str())?;
        paths::join(&self.heads_dir(), &format!("{}.json", path.as_str()))
    }

    pub fn revision_file(&self, revision: &brains_domain::HexId) -> PathBuf {
        self.revisions_dir().join(format!("{revision}.json"))
    }
}

/// Windows serializes namespace writes because distinct logical paths may alias
/// the same directory entry. Unix keeps independent digest-named path locks.
pub fn lock_file(locks_dir: &Path, logical_path: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let _ = logical_path;
        locks_dir.join("namespace.lock")
    }
    #[cfg(not(windows))]
    {
        let digest = Sha256::digest(logical_path.as_bytes());
        locks_dir.join(format!("{}.lock", hex::encode(digest)))
    }
}

pub(crate) fn read_json_file(path: &Path) -> DomainResult<serde_json::Value> {
    let meta = std::fs::metadata(path)
        .map_err(|e| DomainError::StorageUnavailable(format!("{}: {e}", path.display())))?;
    if meta.len() > 64 * 1024 {
        return Err(DomainError::StorageUnavailable(format!(
            "oversized JSON record: {}",
            path.display()
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| DomainError::StorageUnavailable(format!("{}: {e}", path.display())))?;
    serde_json::from_slice(&bytes).map_err(|e| {
        DomainError::StorageUnavailable(format!("corrupt JSON {}: {e}", path.display()))
    })
}

/// Write bytes to a sibling temp file, fsync, then atomically replace
/// `target` (design §9.3 steps 3–4).
pub(crate) fn atomic_write(target: &Path, bytes: &[u8]) -> DomainResult<()> {
    use std::io::Write;
    let dir = target
        .parent()
        .ok_or_else(|| DomainError::StorageUnavailable("target has no parent directory".into()))?;
    let temp = dir.join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| DomainError::StorageUnavailable(format!("temp write: {e}")))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| DomainError::StorageUnavailable(format!("temp write: {e}")))?;
    }
    if let Err(e) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(DomainError::StorageUnavailable(format!(
            "atomic replace: {e}"
        )));
    }
    fsync_dir(dir)
}

/// Unix supports syncing directory entries after publishing a file. Windows
/// flushes the staged file, but does not expose the same directory-fsync contract.
pub(crate) fn fsync_dir(dir: &Path) -> DomainResult<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| DomainError::StorageUnavailable(format!("fsync dir: {e}")))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

/// Atomically publish immutable bytes without replacing an existing object.
pub(crate) fn create_immutable(target: &Path, bytes: &[u8]) -> DomainResult<()> {
    use std::io::Write;
    let dir = target
        .parent()
        .ok_or_else(|| DomainError::Internal("immutable target without parent".into()))?;
    std::fs::create_dir_all(dir).map_err(|e| DomainError::StorageUnavailable(e.to_string()))?;
    let temp = dir.join(format!(".staging-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::hard_link(&temp, target)
    })();
    let _ = std::fs::remove_file(&temp);
    result.map_err(|error| {
        DomainError::StorageUnavailable(format!("immutable publish {}: {error}", target.display()))
    })?;
    fsync_dir(dir)
}
