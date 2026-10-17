//! Tracked working copy (issue #21): checkout, status, pull and push between
//! a local directory and the store, composed from existing APIs — no new
//! server endpoint. The manifest (`.brain/working-copy.json`) records each
//! path's base revision and content hash; it contains no credentials.
//!
//! Conflicts are reported, never overwritten: push uses `--if-revision` from
//! the manifest (create-only for new files, tombstone for deletions); a
//! rejected precondition leaves the file as a conflict and never retries with
//! a fresh revision. Pull updates only unmodified files and reports conflicts
//! for files changed on both sides, using the same three-way text merge as
//! `sync` where it is safe.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::endpoint::FileEndpoint;
use crate::{ClientError, ClientResult, Precondition};

const MANIFEST_FORMAT: &str = "brains-working-copy";
const MANIFEST_VERSION: u32 = 1;
const MANIFEST_FILE: &str = ".brain/working-copy.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BaseEntry {
    revision: String,
    sha256: String,
    #[serde(default)]
    state: String, // "live" | "trashed"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    #[serde(default)]
    prefix: String,
    files: BTreeMap<String, BaseEntry>,
}

impl Manifest {
    fn empty(prefix: &str) -> Self {
        Self {
            format: MANIFEST_FORMAT.into(),
            version: MANIFEST_VERSION,
            prefix: prefix.to_owned(),
            files: BTreeMap::new(),
        }
    }
}

/// One row of `status` output. `Local`/`Store` describe what changed since
/// checkout; both set = conflict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Local,
    Store,
}

#[derive(Clone, Debug, Serialize)]
pub struct StatusRow {
    pub path: String,
    pub local: Option<SideChange>,
    pub store: Option<SideChange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SideChange {
    Modified,
    Added,
    Deleted,
}

#[derive(Default, Serialize)]
pub struct StatusReport {
    pub rows: Vec<StatusRow>,
    pub clean: usize,
}

#[derive(Default, Serialize)]
pub struct PullReport {
    pub updated: usize,
    pub merged: usize,
    pub conflicts: Vec<String>,
    /// Paths trashed in the store and removed locally on pull.
    pub removed: usize,
}

#[derive(Default, Serialize)]
pub struct PushReport {
    pub written: usize,
    pub trashed: usize,
    pub conflicts: Vec<String>,
}

fn io(error: std::io::Error) -> ClientError {
    ClientError::transport(format!("working copy: {error}"))
}

fn invalid(message: &str) -> ClientError {
    ClientError {
        status: 422,
        code: Some("WORKING_COPY_INVALID".into()),
        message: message.to_owned(),
        current_revision: None,
    }
}

fn read_manifest(root: &Path) -> ClientResult<Manifest> {
    let bytes = crate::import::read_regular(root, &root.join(MANIFEST_FILE))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| invalid(&format!("manifest: {e}")))?;
    if manifest.format != MANIFEST_FORMAT || manifest.version != MANIFEST_VERSION {
        return Err(invalid(
            "unsupported working-copy manifest; re-run checkout",
        ));
    }
    Ok(manifest)
}

fn write_manifest(root: &Path, manifest: &Manifest) -> ClientResult<()> {
    let path = root.join(MANIFEST_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let bytes =
        serde_json::to_vec_pretty(manifest).map_err(|e| ClientError::transport(e.to_string()))?;
    std::fs::write(path, bytes).map_err(io)?;
    Ok(())
}

fn require_manifest(root: &Path) -> ClientResult<Manifest> {
    if !root.join(MANIFEST_FILE).is_file() {
        return Err(invalid(&format!(
            "no working copy here (`{MANIFEST_FILE}` missing); run `bsx checkout` first"
        )));
    }
    read_manifest(root)
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Files in the working directory (excluding `.brain/`), vault-relative.
fn local_paths(root: &Path) -> ClientResult<BTreeMap<String, PathBuf>> {
    let mut map = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).map_err(io)? {
            let entry = entry.map_err(io)?;
            if directory == root && entry.file_name() == ".brain" {
                continue;
            }
            let path = entry.path();
            if entry.file_type().map_err(io)?.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|e| invalid(&e.to_string()))?;
                let logical = relative
                    .to_str()
                    .ok_or_else(|| invalid("working-copy path is not UTF-8"))?
                    .replace(std::path::MAIN_SEPARATOR, "/");
                map.insert(logical, path);
            }
        }
    }
    Ok(map)
}

/// `bsx checkout <dir> [--prefix <p>]`: write current files plus the manifest.
pub fn checkout(
    endpoint: &dyn FileEndpoint,
    output: &Path,
    prefix: Option<&str>,
) -> ClientResult<usize> {
    if output.join(MANIFEST_FILE).exists() {
        return Err(invalid(
            "output already holds a working copy; choose a new directory",
        ));
    }
    let prefix = brains_domain::path::normalize_prefix(prefix).unwrap_or("");
    let files = endpoint.current_files()?;
    let mut manifest = Manifest::empty(prefix);
    let mut written = 0usize;
    for entry in &files {
        if entry.state != "live" {
            continue;
        }
        if !prefix.is_empty() && !brains_domain::path::prefix_has(prefix, &entry.path) {
            continue;
        }
        let bytes = endpoint.revision_bytes(&entry.path, &entry.revision)?;
        let target = output.join(&entry.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&target, &bytes).map_err(io)?;
        manifest.files.insert(
            entry.path.clone(),
            BaseEntry {
                revision: entry.revision.clone(),
                sha256: hash(&bytes),
                state: "live".into(),
            },
        );
        written += 1;
    }
    write_manifest(output, &manifest)?;
    Ok(written)
}

/// Compare the working directory, the manifest and the store.
pub fn status(endpoint: &dyn FileEndpoint, root: &Path) -> ClientResult<StatusReport> {
    let manifest = require_manifest(root)?;
    let local = local_paths(root)?;
    let store: BTreeMap<_, _> = endpoint
        .current_files()?
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    let mut report = StatusReport::default();
    let mut paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    paths.extend(local.keys().cloned());
    paths.extend(manifest.files.keys().cloned());
    paths.extend(store.keys().cloned());
    for path in paths {
        if !manifest.prefix.is_empty() && !brains_domain::path::prefix_has(&manifest.prefix, &path)
        {
            continue;
        }
        let base = manifest.files.get(&path);
        let on_disk = local.get(&path);
        let in_store = store.get(&path);
        let local_change = match (base, on_disk) {
            (None, Some(_)) => Some(SideChange::Added),
            (Some(base), Some(disk_path)) => {
                let bytes = std::fs::read(disk_path).map_err(io)?;
                (hash(&bytes) != base.sha256).then_some(SideChange::Modified)
            }
            (Some(base), None) => (base.state == "live").then_some(SideChange::Deleted),
            (None, None) => None,
        };
        let store_change = match (base, in_store) {
            (None, Some(_)) => Some(SideChange::Added),
            (Some(base), Some(entry)) => {
                (entry.revision != base.revision).then_some(SideChange::Modified)
            }
            (Some(base), None) => (base.state == "live").then_some(SideChange::Deleted),
            (None, None) => None,
        };
        match (local_change, store_change) {
            (None, None) => report.clean += 1,
            (local, store_change) => report.rows.push(StatusRow {
                path,
                local,
                store: store_change,
            }),
        }
    }
    Ok(report)
}

/// `bsx pull`: fast-forward unmodified files; three-way merge where both
/// sides changed text and it is safe; conflicts are reported, never forced.
pub fn pull(endpoint: &dyn FileEndpoint, root: &Path, dry_run: bool) -> ClientResult<PullReport> {
    let mut manifest = require_manifest(root)?;
    let report = status(endpoint, root)?;
    let mut result = PullReport::default();
    let store: BTreeMap<_, _> = endpoint
        .current_files()?
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    let local = local_paths(root)?;
    for row in &report.rows {
        let Some(base) = manifest.files.get(&row.path).cloned() else {
            // Added on both sides with no common base: a conflict, reported
            // like any other both-sides change.
            if row.local == Some(SideChange::Added) && row.store == Some(SideChange::Added) {
                result.conflicts.push(row.path.clone());
                continue;
            }
            // New in the store, unknown locally: materialize it.
            if let Some(entry) = store.get(&row.path)
                && entry.state == "live"
                && row.local.is_none()
            {
                if !dry_run {
                    let bytes = endpoint.revision_bytes(&row.path, &entry.revision)?;
                    let target = root.join(&row.path);
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).map_err(io)?;
                    }
                    std::fs::write(&target, &bytes).map_err(io)?;
                    manifest.files.insert(
                        row.path.clone(),
                        BaseEntry {
                            revision: entry.revision.clone(),
                            sha256: hash(&bytes),
                            state: "live".into(),
                        },
                    );
                }
                result.updated += 1;
            }
            continue;
        };
        let store_entry = store.get(&row.path);
        match (&row.local, &row.store) {
            // Deleted locally, changed in store → conflict.
            (Some(SideChange::Deleted), Some(_)) => result.conflicts.push(row.path.clone()),
            // Changed locally, deleted in store → conflict.
            (Some(_), Some(SideChange::Deleted)) => result.conflicts.push(row.path.clone()),
            // Only the store moved: fast-forward the file.
            (None, Some(_)) => match store_entry {
                Some(entry) if entry.state == "live" => {
                    if !dry_run {
                        let bytes = endpoint.revision_bytes(&row.path, &entry.revision)?;
                        let target = root.join(&row.path);
                        if let Some(parent) = target.parent() {
                            std::fs::create_dir_all(parent).map_err(io)?;
                        }
                        std::fs::write(&target, &bytes).map_err(io)?;
                        manifest.files.insert(
                            row.path.clone(),
                            BaseEntry {
                                revision: entry.revision.clone(),
                                sha256: hash(&bytes),
                                state: "live".into(),
                            },
                        );
                    }
                    result.updated += 1;
                }
                // Trashed (or purged) in store: remove the local file.
                _ => {
                    if !dry_run {
                        if let Some(disk) = local.get(&row.path) {
                            std::fs::remove_file(disk).map_err(io)?;
                        }
                        manifest.files.insert(
                            row.path.clone(),
                            BaseEntry {
                                revision: store_entry
                                    .map(|entry| entry.revision.clone())
                                    .unwrap_or_default(),
                                sha256: String::new(),
                                state: "trashed".into(),
                            },
                        );
                    }
                    result.removed += 1;
                }
            },
            // Both changed: three-way text merge where a base blob survives.
            (Some(SideChange::Modified), Some(SideChange::Modified)) => {
                let Some(entry) = store_entry.filter(|entry| entry.state == "live") else {
                    result.conflicts.push(row.path.clone());
                    continue;
                };
                let Some(disk_path) = local.get(&row.path) else {
                    result.conflicts.push(row.path.clone());
                    continue;
                };
                let (store_bytes, _) =
                    crate::endpoint::read_verified(endpoint, &row.path, &entry.revision)?;
                let merged = merge_with_base(
                    endpoint,
                    &row.path,
                    &base,
                    &std::fs::read(disk_path).map_err(io)?,
                    &store_bytes,
                );
                match merged {
                    Ok(bytes) => {
                        if !dry_run {
                            std::fs::write(disk_path, &bytes).map_err(io)?;
                            // The merged file is committed on the next push;
                            // the manifest base follows the store revision so
                            // status stays meaningful until then.
                            manifest.files.insert(
                                row.path.clone(),
                                BaseEntry {
                                    revision: entry.revision.clone(),
                                    sha256: hash(&store_bytes),
                                    state: "live".into(),
                                },
                            );
                        }
                        result.merged += 1;
                    }
                    Err(_) => result.conflicts.push(row.path.clone()),
                }
            }
            // Only the local side moved, or nothing to do.
            _ => {}
        }
    }
    if !dry_run {
        write_manifest(root, &manifest)?;
    }
    Ok(result)
}

/// Three-way merge: ancestor = manifest base blob, ours = local file,
/// theirs = store bytes. Binary or missing base → Err (reported as conflict).
fn merge_with_base(
    endpoint: &dyn FileEndpoint,
    path: &str,
    base: &BaseEntry,
    ours: &[u8],
    theirs: &[u8],
) -> ClientResult<Vec<u8>> {
    let (ancestor, content) = crate::endpoint::read_verified(endpoint, path, &base.revision)?;
    if !brains_domain::is_text_media(&content.media_type) {
        return Err(invalid("binary files never merge"));
    }
    let (Ok(ancestor), Ok(ours), Ok(theirs)) = (
        std::str::from_utf8(&ancestor),
        std::str::from_utf8(ours),
        std::str::from_utf8(theirs),
    ) else {
        return Err(invalid("binary files never merge"));
    };
    diffy::merge(ancestor, ours, theirs)
        .map(|merged| merged.into_bytes())
        .map_err(|_| invalid("overlapping text edits"))
}

/// `bsx push`: write local changes with the manifest's CAS preconditions
/// (create-only for new files, tombstone for deletions), then refresh the
/// manifest. A rejected precondition is a conflict; never retried with a
/// fresh revision.
pub fn push(endpoint: &dyn FileEndpoint, root: &Path, dry_run: bool) -> ClientResult<PushReport> {
    let mut manifest = require_manifest(root)?;
    let report = status(endpoint, root)?;
    let mut result = PushReport::default();
    let store: BTreeMap<_, _> = endpoint
        .current_files()?
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    let local = local_paths(root)?;
    for row in &report.rows {
        let Some(local_change) = &row.local else {
            continue;
        };
        let base = manifest.files.get(&row.path);
        // Pushing against a moved base is a conflict: pull or merge first.
        // The single safe exception is deleting a file the store also
        // deleted — nothing to write either way.
        if row.store.is_some()
            && !matches!(
                (&row.local, &row.store),
                (Some(SideChange::Deleted), Some(SideChange::Deleted))
            )
        {
            result.conflicts.push(row.path.clone());
            continue;
        }
        let outcome: ClientResult<bool> = match local_change {
            SideChange::Added => {
                let bytes = std::fs::read(
                    local
                        .get(&row.path)
                        .ok_or_else(|| invalid("added file vanished"))?,
                )
                .map_err(io)?;
                if dry_run {
                    Ok(false)
                } else {
                    endpoint.put(
                        &row.path,
                        &bytes,
                        brains_domain::media_type_for(&row.path),
                        crate::import::is_executable(
                            local
                                .get(&row.path)
                                .ok_or_else(|| invalid("added file vanished"))?,
                        )?,
                        Precondition::Create,
                    )?;
                    manifest.files.insert(
                        row.path.clone(),
                        BaseEntry {
                            revision: store
                                .get(&row.path)
                                .map(|entry| entry.revision.clone())
                                .unwrap_or_default(),
                            sha256: hash(&bytes),
                            state: "live".into(),
                        },
                    );
                    Ok(true)
                }
            }
            SideChange::Modified => {
                let Some(base) = base else {
                    return Err(invalid(&format!(
                        "{}: no base revision to update",
                        row.path
                    )));
                };
                let disk_path = local
                    .get(&row.path)
                    .ok_or_else(|| invalid("modified file vanished"))?;
                let bytes = std::fs::read(disk_path).map_err(io)?;
                if dry_run {
                    Ok(false)
                } else {
                    let mutation = endpoint.put(
                        &row.path,
                        &bytes,
                        brains_domain::media_type_for(&row.path),
                        crate::import::is_executable(disk_path)?,
                        Precondition::IfRevision(base.revision.clone()),
                    )?;
                    manifest.files.insert(
                        row.path.clone(),
                        BaseEntry {
                            revision: mutation.entry.revision.clone(),
                            sha256: hash(&bytes),
                            state: "live".into(),
                        },
                    );
                    Ok(true)
                }
            }
            SideChange::Deleted => {
                let Some(base) = base else {
                    continue;
                };
                if dry_run {
                    Ok(false)
                } else if base.state == "live" {
                    let mutation = endpoint.trash(&row.path, &base.revision)?;
                    manifest.files.insert(
                        row.path.clone(),
                        BaseEntry {
                            revision: mutation.entry.revision.clone(),
                            sha256: String::new(),
                            state: "trashed".into(),
                        },
                    );
                    Ok(true)
                } else {
                    // The store side purged the tombstone: the entry is gone
                    // from every listing. Nothing to write; settle the local
                    // base so the row leaves status for good.
                    manifest.files.insert(
                        row.path.clone(),
                        BaseEntry {
                            revision: String::new(),
                            sha256: String::new(),
                            state: "trashed".into(),
                        },
                    );
                    Ok(false)
                }
            }
        };
        match outcome {
            Ok(_) => match local_change {
                SideChange::Deleted => result.trashed += 1,
                _ => result.written += 1,
            },
            Err(error) if matches!(error.status, 409 | 412) => {
                // CAS rejected: leave the row as a conflict for the next
                // status/pull; the manifest base is untouched.
                result.conflicts.push(row.path.clone());
            }
            Err(error) => {
                // Persist what already landed so a rerun resumes instead of
                // reporting ghost conflicts for committed paths.
                if !dry_run {
                    write_manifest(root, &manifest)?;
                }
                return Err(error);
            }
        }
    }
    if !dry_run {
        write_manifest(root, &manifest)?;
    }
    Ok(result)
}
