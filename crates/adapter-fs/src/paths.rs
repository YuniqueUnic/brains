//! Filesystem constraints stay at the native storage boundary. Remote logical
//! paths keep the same contract on every client platform.

use std::path::{Path, PathBuf};

#[cfg(windows)]
use brains_domain::DomainError;
use brains_domain::DomainResult;

pub(crate) fn validate_name(logical: &str) -> DomainResult<()> {
    #[cfg(windows)]
    for segment in logical.split('/') {
        brains_domain::path::check_portable_segment(segment)
            .map_err(DomainError::InvalidArgument)?;
    }
    #[cfg(not(windows))]
    let _ = logical;
    Ok(())
}

pub(crate) fn join(root: &Path, logical: &str) -> DomainResult<PathBuf> {
    validate_name(logical)?;
    Ok(root.join(logical))
}

/// Called while holding the namespace write lock. Reads check HEAD.path without
/// scanning the directory, so listing a large vault stays linear in its files.
pub(crate) fn check_spelling(root: &Path, logical: &str) -> DomainResult<()> {
    #[cfg(windows)]
    {
        // Windows resolves case aliases to the same file. Reject a different
        // spelling before creating a HEAD whose path could not be listed back.
        let mut parent = root.to_path_buf();
        for segment in logical.split('/') {
            let entries = match std::fs::read_dir(&parent) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(e) => return Err(DomainError::StorageUnavailable(e.to_string())),
            };
            let candidate = parent.join(segment);
            if candidate
                .try_exists()
                .map_err(|e| DomainError::StorageUnavailable(e.to_string()))?
            {
                let mut exact = false;
                for entry in entries {
                    let entry =
                        entry.map_err(|e| DomainError::StorageUnavailable(e.to_string()))?;
                    if entry.file_name() == std::ffi::OsStr::new(segment) {
                        exact = true;
                        break;
                    }
                }
                if !exact {
                    return Err(DomainError::InvalidArgument(format!(
                        "path spelling conflicts with an existing Windows file: {logical}"
                    )));
                }
            }
            parent = candidate;
        }
    }
    #[cfg(not(windows))]
    let _ = (root, logical);
    Ok(())
}
