//! Working-tree export over HTTP primitives (design §11.1, §14.4): paginate
//! authoritative heads, download each live file, verify digests, apply the
//! portability checks, publish atomically with a manifest. Per-file
//! consistency — never a whole-vault snapshot claim.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use brains_domain::path::check_portable_segment;

use crate::endpoint::{FileEndpoint, read_verified};
use crate::{ClientError, ClientResult};

#[derive(Serialize, Deserialize)]
pub(crate) struct ManifestEntry {
    pub path: String,
    pub revision: String,
    pub generation: i64,
    pub sha256: String,
    pub size: i64,
    pub executable_restored: bool,
    pub executable: bool,
    pub media_type: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ManifestTombstone {
    pub path: String,
    pub revision: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub vault: String,
    pub consistency: String,
    pub started_at: String,
    pub finished_at: String,
    pub history_verified: bool,
    pub entries: Vec<ManifestEntry>,
    pub tombstones: Vec<ManifestTombstone>,
}

/// Export the current working tree to a NEW directory `output`.
/// Two phases (design §14.4): enumerate the authoritative listing to the
/// end FIRST, then download each entry's bytes from its recorded immutable
/// revision — a concurrent writer mid-export can neither corrupt a file nor
/// desynchronize the manifest. Fails loudly on collision or portability
/// problems instead of dropping a file (design §6.2, U16).
pub fn export_working_tree(
    client: &dyn FileEndpoint,
    output: &std::path::Path,
    vault: &str,
) -> ClientResult<String> {
    publish_directory(output, |staging| {
        let manifest = export_into_staging(client, staging, vault, now_rfc3339())?;
        Ok(serde_json::json!({"output": output, "files": manifest.entries.len(), "tombstones": manifest.tombstones.len()}).to_string())
    })
}

/// Shared staging lifecycle for both portable formats; failures leave no output.
pub(crate) fn publish_directory(
    output: &std::path::Path,
    write: impl FnOnce(&std::path::Path) -> ClientResult<String>,
) -> ClientResult<String> {
    if output.symlink_metadata().is_ok() {
        return Err(crate::endpoint::invalid("output already exists"));
    }
    let parent = output
        .parent()
        .ok_or_else(|| crate::endpoint::invalid("output needs a parent directory"))?;
    let staging = parent.join(format!(
        ".brains-export-{}",
        brains_domain::HexId::generate()
    ));
    std::fs::create_dir_all(&staging).map_err(io_err)?;
    let result = (|| {
        let report = write(&staging)?;
        if output.symlink_metadata().is_ok() {
            return Err(crate::endpoint::invalid("output appeared during export"));
        }
        std::fs::rename(&staging, output).map_err(io_err)?;
        Ok(report)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn export_into_staging(
    client: &dyn FileEndpoint,
    staging: &std::path::Path,
    vault: &str,
    started_at: String,
) -> ClientResult<Manifest> {
    // Phase 1: enumerate the authoritative listing to the end.
    let listed = client.current_files()?;
    check_portability(
        &listed
            .iter()
            .filter(|entry| entry.state == "live")
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
    )?;

    // Phase 2: download each entry's recorded immutable revision.
    let mut entries: Vec<ManifestEntry> = Vec::new();
    let mut tombstones: Vec<ManifestTombstone> = Vec::new();
    let mut directories = HashSet::new();
    for entry in &listed {
        if entry.state == "trashed" {
            tombstones.push(ManifestTombstone {
                path: entry.path.clone(),
                revision: entry.revision.clone(),
            });
            continue;
        }
        // The authoritative digest lives on the revision record (store
        // listing entries carry none): fetch it, download the immutable
        // bytes, and refuse any mismatch — the §14.5 recovery premise.
        let (bytes, content) = read_verified(client, &entry.path, &entry.revision)?;
        write_entry_file(staging, &entry.path, &bytes, &mut directories)?;
        restore_executable(&staging.join(&entry.path), content.executable)?;
        entries.push(ManifestEntry {
            path: entry.path.clone(),
            revision: entry.revision.clone(),
            generation: entry.generation,
            sha256: content.sha256,
            size: bytes.len() as i64,
            executable_restored: cfg!(unix),
            executable: content.executable,
            media_type: content.media_type,
        });
    }

    let manifest = Manifest {
        format: "brains-working-tree".into(),
        format_version: 2,
        vault: vault.to_owned(),
        consistency: "per_file".into(),
        started_at,
        finished_at: now_rfc3339(),
        history_verified: false,
        entries,
        tombstones,
    };
    let brain_dir = staging.join(".brain");
    std::fs::create_dir(&brain_dir).map_err(io_err)?;
    std::fs::write(
        brain_dir.join("export-manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|e| ClientError::transport(e.to_string()))?,
    )
    .map_err(io_err)?;
    Ok(manifest)
}

fn write_entry_file(
    root: &std::path::Path,
    logical: &str,
    bytes: &[u8],
    directories: &mut HashSet<std::path::PathBuf>,
) -> ClientResult<()> {
    use std::io::Write;

    // Unicode folding is a portability preflight, not a model of every OS.
    // Only reuse directories created for this exact spelling. The filesystem
    // must reject any additional alias rather than silently merge two paths.
    if let Some((parent, _)) = logical.rsplit_once('/') {
        let mut directory = root.to_path_buf();
        for part in parent.split('/') {
            directory.push(part);
            if directories.insert(directory.clone()) {
                std::fs::create_dir(&directory).map_err(io_err)?;
            }
        }
    }
    let target = root.join(logical);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(io_err)
}

/// The checks export_raw.py also applies: reserved names, portable segments,
/// case-fold collisions, file/dir conflicts.
pub(crate) fn check_portability(paths: &[String]) -> ClientResult<()> {
    let mut folded_dirs: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut folded_files: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for path in paths {
        brains_domain::VaultPath::new(path)
            .map_err(|error| crate::endpoint::invalid(&error.to_string()))?;
        let parts: Vec<&str> = path.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            check_portable_segment(part).map_err(|reason| ClientError {
                status: 422,
                code: Some("INVALID_FORMAT".into()),
                message: format!("path `{path}` is not portable: {reason}"),
                current_revision: None,
            })?;
            let original = parts[..=index].join("/");
            let folded = unicase::UniCase::new(&original).to_folded_case();
            if index + 1 < parts.len() {
                if let Some(existing) = folded_files.get(&folded) {
                    return Err(collide(existing, &original));
                }
                if let Some(existing) = folded_dirs.get(&folded) {
                    if existing != &original {
                        return Err(collide(existing, &original));
                    }
                } else {
                    folded_dirs.insert(folded, original);
                }
            } else {
                if folded_dirs.contains_key(&folded) {
                    return Err(collide(folded_dirs.get(&folded).unwrap(), &original));
                }
                if let Some(existing) = folded_files.get(&folded) {
                    return Err(collide(existing, &original));
                }
                folded_files.insert(folded, original);
            }
        }
    }
    Ok(())
}

fn collide(a: &str, b: &str) -> ClientError {
    ClientError {
        status: 422,
        code: Some("INVALID_FORMAT".into()),
        message: format!("path collision on export: `{a}` vs `{b}`"),
        current_revision: None,
    }
}

fn io_err(e: std::io::Error) -> ClientError {
    ClientError::transport(format!("export io: {e}"))
}

fn now_rfc3339() -> String {
    // The client keeps timestamps minimal: the server owns authoritative time.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    time::OffsetDateTime::from_unix_timestamp(secs as i64)
        .ok()
        .and_then(|t| {
            t.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default()
}

fn restore_executable(path: &std::path::Path, executable: bool) -> ClientResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).map_err(io_err)?.permissions();
        let mode = permissions.mode();
        permissions.set_mode(if executable {
            mode | 0o111
        } else {
            mode & !0o111
        });
        std::fs::set_permissions(path, permissions).map_err(io_err)?;
    }
    #[cfg(not(unix))]
    let _ = (path, executable);
    Ok(())
}
