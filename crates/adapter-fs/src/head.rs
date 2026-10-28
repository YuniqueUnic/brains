//! HEAD storage with cross-process compare-and-swap (design §9.3).

use std::io::Seek;

use fs2::FileExt;

use brains_domain::{DomainError, DomainResult, Head, VaultPath};
use brains_ports::{CasOutcome, ExpectedHead, HeadStore, StoredHead};

use crate::{FsStore, atomic_write, lock_file, read_json_file};

pub struct FsHeadStore {
    store: FsStore,
}

impl FsHeadStore {
    pub fn new(store: FsStore) -> Self {
        Self { store }
    }
}

fn parse_head(value: serde_json::Value) -> DomainResult<Head> {
    serde_json::from_value::<Head>(value)
        .map_err(|e| DomainError::StorageUnavailable(format!("corrupt HEAD record: {e}")))
}

impl FsHeadStore {
    fn read_current(&self, path: &VaultPath) -> DomainResult<Option<StoredHead>> {
        let file = self.store.head_file(path);
        if !file.exists() {
            return Ok(None);
        }
        // R2 raw backups retain zero-byte, conditionally written vacancy markers.
        if std::fs::metadata(&file)
            .map_err(|e| DomainError::StorageUnavailable(e.to_string()))?
            .len()
            == 0
        {
            return Ok(None);
        }
        let head = parse_head(read_json_file(&file)?)?;
        head.validate()?;
        Ok(Some(StoredHead {
            storage_token: head.revision.as_str().to_owned(),
            head,
        }))
    }
}

impl HeadStore for FsHeadStore {
    async fn get(&self, path: &VaultPath) -> DomainResult<Option<StoredHead>> {
        self.read_current(path)
    }

    /// Lock → verify token → temp write → atomic replace → unlock.
    /// All writers share this protocol, so the lock serializes HEAD state.
    async fn cas(
        &self,
        path: &VaultPath,
        expected: &ExpectedHead,
        new_head: &Head,
    ) -> DomainResult<CasOutcome> {
        new_head.validate()?;
        let target = self.store.head_file(path);
        let lock_path = lock_file(&self.store.locks_dir(), path.as_str());
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DomainError::StorageUnavailable(format!("locks dir: {e}")))?;
        }
        let mut lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| DomainError::StorageUnavailable(format!("lock open: {e}")))?;
        lock.lock_exclusive()
            .map_err(|e| DomainError::StorageUnavailable(format!("lock acquire: {e}")))?;
        let outcome = self.under_lock(path, &target, expected, new_head);
        let _ = lock.seek(std::io::SeekFrom::Start(0));
        let _ = lock.unlock();
        outcome
    }

    /// Lock → verify token → remove the file → unlock (mirror of `cas`).
    async fn remove(&self, path: &VaultPath, expected: &ExpectedHead) -> DomainResult<CasOutcome> {
        let target = self.store.head_file(path);
        let lock_path = lock_file(&self.store.locks_dir(), path.as_str());
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DomainError::StorageUnavailable(format!("locks dir: {e}")))?;
        }
        let mut lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| DomainError::StorageUnavailable(format!("lock open: {e}")))?;
        lock.lock_exclusive()
            .map_err(|e| DomainError::StorageUnavailable(format!("lock acquire: {e}")))?;
        let matches = match (self.read_current(path)?, expected) {
            (None, _) => true, // already gone — removal achieved
            (Some(current), ExpectedHead::StorageToken(token)) => current.storage_token == *token,
            (Some(_), ExpectedHead::Absent) => false,
        };
        let outcome = if matches {
            match std::fs::remove_file(&target) {
                Ok(()) => CasOutcome::Committed,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => CasOutcome::Committed,
                Err(e) => Err(DomainError::StorageUnavailable(format!(
                    "head remove {}: {e}",
                    target.display()
                )))?,
            }
        } else {
            CasOutcome::PreconditionFailed
        };
        let _ = lock.seek(std::io::SeekFrom::Start(0));
        let _ = lock.unlock();
        Ok(outcome)
    }

    async fn list(
        &self,
        prefix: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> DomainResult<Vec<StoredHead>> {
        let mut logical: Vec<String> = Vec::new();
        collect_head_paths(&self.store.heads_dir(), &mut String::new(), &mut logical)?;
        logical.sort();
        let mut out = Vec::new();
        for key in logical {
            if let Some(after) = after
                && key.as_str() <= after
            {
                continue;
            }
            if let Some(prefix) = prefix
                && !brains_domain::path::prefix_has(prefix, &key)
            {
                continue;
            }
            if out.len() >= limit {
                break;
            }
            let path = match VaultPath::new(&key) {
                Ok(p) => p,
                Err(_) => continue, // stray file in heads/ never blocks listing
            };
            if let Some(stored) = self.read_current(&path)? {
                out.push(stored);
            }
        }
        Ok(out)
    }
}

impl FsHeadStore {
    fn under_lock(
        &self,
        path: &VaultPath,
        target: &std::path::Path,
        expected: &ExpectedHead,
        new_head: &Head,
    ) -> DomainResult<CasOutcome> {
        let matches = match (self.read_current(path)?, expected) {
            (None, ExpectedHead::Absent) => true,
            (None, ExpectedHead::StorageToken(_)) => false,
            (Some(current), ExpectedHead::StorageToken(token)) => current.storage_token == *token,
            (Some(_), ExpectedHead::Absent) => false,
        };
        if !matches {
            return Ok(CasOutcome::PreconditionFailed);
        }
        let bytes = serde_json::to_vec(new_head)
            .map_err(|e| DomainError::Internal(format!("HEAD serialization: {e}")))?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DomainError::StorageUnavailable(format!("heads dir: {e}")))?;
        }
        atomic_write(target, &bytes)?;
        Ok(CasOutcome::Committed)
    }
}

fn collect_head_paths(
    dir: &std::path::Path,
    relative: &mut String,
    out: &mut Vec<String>,
) -> DomainResult<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(DomainError::StorageUnavailable(format!(
                "heads dir {}: {e}",
                dir.display()
            )));
        }
    };
    let mut names: Vec<std::ffi::OsString> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    names.sort();
    for name in names {
        let name = name.to_string_lossy().into_owned();
        let child_rel = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        let child_path = dir.join(&name);
        if child_path.is_dir() {
            collect_head_paths(&child_path, &mut child_rel.clone(), out)?;
        } else if let Some(key) = child_rel.strip_suffix(".json")
            && std::fs::metadata(&child_path)
                .map_err(|e| DomainError::StorageUnavailable(e.to_string()))?
                .len()
                > 0
        {
            out.push(key.to_owned());
        }
    }
    Ok(())
}
