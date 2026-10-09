//! Import editable working trees. Missing source files never delete targets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::endpoint::{FileEndpoint, invalid, read_verified};
use crate::export::{Manifest, check_portability};
use crate::{ClientError, ClientResult, Precondition};

#[derive(Default, serde::Serialize)]
pub struct ImportReport {
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub conflicts: Vec<String>,
    pub index_pending: Vec<String>,
    pub failures: Vec<String>,
    pub dry_run: bool,
}

impl ImportReport {
    pub fn success(&self) -> bool {
        self.conflicts.is_empty() && self.failures.is_empty() && self.index_pending.is_empty()
    }
}

pub fn import_working_tree(
    endpoint: &dyn FileEndpoint,
    root: &Path,
    overwrite: bool,
    dry_run: bool,
) -> ClientResult<ImportReport> {
    let paths = source_paths(root)?;
    check_portability(&paths)?;
    let manifest_path = root.join(".brain/export-manifest.json");
    let metadata: BTreeMap<_, _> = if manifest_path.exists() {
        let manifest: Manifest = serde_json::from_slice(&read_regular(root, &manifest_path)?)
            .map_err(|error| invalid(&format!("invalid export manifest: {error}")))?;
        if manifest.format != "brains-working-tree" || manifest.format_version != 2 {
            return Err(invalid(
                "unsupported working-tree manifest; export with the current bsx first",
            ));
        }
        manifest
            .entries
            .into_iter()
            .map(|entry| (entry.path.clone(), entry))
            .collect()
    } else {
        BTreeMap::new()
    };
    let targets: BTreeMap<_, _> = endpoint
        .current_files()?
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    let mut report = ImportReport {
        dry_run,
        ..Default::default()
    };
    for path in paths {
        let outcome = (|| -> ClientResult<()> {
            let disk_path = root.join(&path);
            let bytes = read_regular(root, &disk_path)?;
            let media = metadata
                .get(&path)
                .map(|entry| entry.media_type.as_str())
                .unwrap_or_else(|| brains_domain::media_type_for(&path));
            // On Unix the editable file's mode wins. Else retain the exported
            // flag since those filesystems cannot represent Unix execute bits.
            let executable = if cfg!(unix) {
                is_executable(&disk_path)?
            } else {
                metadata.get(&path).is_some_and(|entry| entry.executable)
            };
            let target = targets.get(&path);
            let precondition = match target {
                None => Precondition::Create,
                Some(entry) if entry.state == "live" => {
                    let (existing, content) = read_verified(endpoint, &path, &entry.revision)?;
                    if bytes == existing
                        && media == content.media_type
                        && executable == content.executable
                    {
                        report.unchanged += 1;
                        return Ok(());
                    }
                    if !overwrite {
                        report.conflicts.push(path.clone());
                        return Ok(());
                    }
                    Precondition::IfRevision(entry.revision.clone())
                }
                Some(_) => {
                    report.conflicts.push(path.clone());
                    return Ok(());
                }
            };
            if !dry_run {
                let result = endpoint.put(&path, &bytes, media, executable, precondition)?;
                if result.index_state != "ready" {
                    report.index_pending.push(path.clone());
                }
            }
            if target.is_none() {
                report.created += 1;
            } else {
                report.updated += 1;
            }
            Ok(())
        })();
        if let Err(error) = outcome {
            report.failures.push(format!("{path}: {error}"));
            // A transport/permission failure must not trigger a request for
            // every remaining path; already-completed writes are reported.
            if !matches!(error.status, 409 | 412) {
                break;
            }
        }
    }
    Ok(report)
}

fn source_paths(root: &Path) -> ClientResult<Vec<String>> {
    let metadata = std::fs::symlink_metadata(root).map_err(io_error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid("import source must be a real directory"));
    }
    let mut directories = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if directory == root && entry.file_name() == ".brain" {
                continue;
            }
            let kind = entry.file_type().map_err(io_error)?;
            let path = entry.path();
            if kind.is_dir() {
                directories.push(path);
            } else if kind.is_file() {
                let root_meta = std::fs::symlink_metadata(root).map_err(io_error)?;
                if !root_meta.is_dir() || root_meta.file_type().is_symlink() {
                    return Err(invalid("source must be a real directory"));
                }
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| invalid(&error.to_string()))?;
                let logical = relative
                    .to_str()
                    .ok_or_else(|| invalid("import path is not UTF-8"))?
                    .replace(std::path::MAIN_SEPARATOR, "/");
                paths.push(logical);
            } else {
                return Err(invalid(&format!(
                    "symlinks and special files cannot be imported: {}",
                    path.display()
                )));
            }
        }
    }
    paths.sort();
    Ok(paths)
}

/// Refuse symlink components before reading data supplied by an archive/tree.
pub(crate) fn read_regular(root: &Path, path: &Path) -> ClientResult<Vec<u8>> {
    let root_meta = std::fs::symlink_metadata(root).map_err(io_error)?;
    if !root_meta.is_dir() || root_meta.file_type().is_symlink() {
        return Err(invalid("source must be a real directory"));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|error| invalid(&error.to_string()))?;
    let mut current = PathBuf::from(root);
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(invalid("non-relative archive path"));
        }
        current.push(component);
        if std::fs::symlink_metadata(&current)
            .map_err(io_error)?
            .file_type()
            .is_symlink()
        {
            return Err(invalid(&format!("refusing symlink: {}", current.display())));
        }
    }
    if !std::fs::metadata(path).map_err(io_error)?.is_file() {
        return Err(invalid("expected a regular file"));
    }
    std::fs::read(path).map_err(io_error)
}

pub fn is_executable(path: &Path) -> ClientResult<bool> {
    let metadata = std::fs::metadata(path).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Ok(false)
    }
}

fn io_error(error: std::io::Error) -> ClientError {
    ClientError::transport(error.to_string())
}
