//! Portable, reachable-history archives. The manifest contains immutable
//! revision metadata; blobs use opaque local names derived from storage keys.
//! Restoration is a sequence of per-generation CAS commits, not a vault-wide
//! transaction. Interrupted restores can resume only from an exact prefix.

mod endpoint;
mod restore;

pub use endpoint::{ArchiveEndpoint, ArchiveTarget};
pub use restore::import_archive;

use crate::{ClientError, ClientResult, endpoint::invalid, import::read_regular};
use brains_domain::{Head, Revision};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Archive {
    format: String,
    format_version: u32,
    vault: String,
    files: Vec<ArchivedFile>,
}

#[derive(Serialize, Deserialize)]
struct ArchivedFile {
    head: Head,
    /// Oldest first, with no gaps; ends at `head.revision`.
    revisions: Vec<Revision>,
}

pub fn export_archive(
    endpoint: &dyn ArchiveEndpoint,
    output: &Path,
    vault: &str,
) -> ClientResult<String> {
    crate::export::publish_directory(output, |staging| {
        let before = endpoint.current_files()?;
        let mut archive = Archive {
            format: "brains-archive".into(),
            format_version: 1,
            vault: vault.into(),
            files: Vec::new(),
        };
        let mut saved = BTreeSet::new();
        std::fs::create_dir(staging.join("blobs")).map_err(io_error)?;
        for entry in &before {
            let mut revisions = Vec::new();
            let mut next = Some(entry.revision.clone());
            let mut seen = BTreeSet::new();
            while let Some(id) = next {
                if !seen.insert(id.clone()) {
                    return Err(invalid("revision parent cycle"));
                }
                let revision = endpoint.archive_revision(&entry.path, &id)?;
                validate_revision(&revision)?;
                if revision.path != entry.path || revision.revision.as_str() != id {
                    return Err(invalid("archive revision identity mismatch"));
                }
                if let Some(content) = &revision.content
                    && saved.insert(content.key.clone())
                {
                    let bytes = endpoint.revision_bytes(&entry.path, &id)?;
                    verify_bytes(content, &bytes)?;
                    std::fs::write(blob_path(staging, &content.key), bytes).map_err(io_error)?;
                }
                next = revision.parent.as_ref().map(ToString::to_string);
                revisions.push(revision);
            }
            revisions.reverse();
            let head = revisions
                .last()
                .ok_or_else(|| invalid("empty revision chain"))?
                .head()
                .map_err(domain_error)?;
            if !matches_entry(&head, entry) {
                return Err(invalid("HEAD differs from its revision"));
            }
            archive.files.push(ArchivedFile { head, revisions });
        }
        validate_archive(&archive, staging)?;
        if endpoint.current_files()? != before {
            return Err(invalid(
                "source changed during archive export; retry after pausing writes",
            ));
        }
        std::fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec_pretty(&archive).map_err(|e| invalid(&e.to_string()))?,
        )
        .map_err(io_error)?;
        Ok(serde_json::json!({"output": output, "paths": archive.files.len(), "revisions": archive.files.iter().map(|f| f.revisions.len()).sum::<usize>(), "blobs": saved.len(), "history_verified": true}).to_string())
    })
}

fn load_archive(root: &Path) -> ClientResult<Archive> {
    let archive: Archive =
        serde_json::from_slice(&read_regular(root, &root.join("manifest.json"))?)
            .map_err(|error| invalid(&format!("invalid archive manifest: {error}")))?;
    validate_archive(&archive, root)?;
    Ok(archive)
}

/// Validate the entire archive before any target mutation. Paths never become
/// disk paths: only validated, hashed blob keys are used to read local bytes.
fn validate_archive(archive: &Archive, root: &Path) -> ClientResult<()> {
    if archive.format != "brains-archive" || archive.format_version != 1 {
        return Err(invalid("unsupported archive format"));
    }
    if archive.vault.is_empty()
        || archive.vault.len() > 32
        || !archive
            .vault
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err(invalid("invalid source vault"));
    }
    let mut paths = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut blobs = BTreeMap::new();
    for file in &archive.files {
        file.head.validate().map_err(domain_error)?;
        if !paths.insert(&file.head.path) {
            return Err(invalid("duplicate archive path"));
        }
        let mut parent = None;
        for (index, revision) in file.revisions.iter().enumerate() {
            validate_revision(revision)?;
            if revision.path != file.head.path
                || revision.generation != index as i64 + 1
                || revision.parent.as_ref() != parent
                || !ids.insert(&revision.revision)
            {
                return Err(invalid(
                    "invalid archive chain, generation, path or duplicate revision",
                ));
            }
            if let Some(content) = &revision.content {
                if !content
                    .key
                    .starts_with(&format!("vaults/{}/blobs/", archive.vault))
                {
                    return Err(invalid("archive blob belongs to another vault"));
                }
                let identity = (&content.sha256, content.size);
                if let Some(previous) = blobs.insert(&content.key, identity) {
                    if previous != identity {
                        return Err(invalid("conflicting archive blob metadata"));
                    }
                } else {
                    verify_bytes(
                        content,
                        &read_regular(root, &blob_path(root, &content.key))?,
                    )?;
                }
            }
            parent = Some(&revision.revision);
        }
        let last = file
            .revisions
            .last()
            .ok_or_else(|| invalid("empty archive chain"))?;
        if last.head().map_err(domain_error)? != file.head {
            return Err(invalid("archive HEAD does not match its final revision"));
        }
    }
    Ok(())
}

fn validate_revision(revision: &Revision) -> ClientResult<()> {
    revision.validate().map_err(domain_error)?;
    if serde_json::to_vec(revision)
        .map_err(|e| invalid(&e.to_string()))?
        .len()
        > 65536
    {
        return Err(invalid("revision metadata exceeds 64 KiB"));
    }
    Ok(())
}

fn matches_entry(head: &Head, entry: &brains_api_types::Entry) -> bool {
    head.path == entry.path
        && head.revision.as_str() == entry.revision
        && head.generation == entry.generation
        && head.state.as_str() == entry.state
        && entry.updated_at.as_deref() == Some(head.updated_at.as_str())
}

fn blob_path(root: &Path, key: &str) -> PathBuf {
    root.join("blobs")
        .join(hex::encode(Sha256::digest(key.as_bytes())))
}
fn verify_bytes(content: &brains_domain::ContentRef, bytes: &[u8]) -> ClientResult<()> {
    if bytes.len() as i64 != content.size || hex::encode(Sha256::digest(bytes)) != content.sha256 {
        return Err(invalid("archive blob checksum or size mismatch"));
    }
    Ok(())
}
fn domain_error(error: brains_domain::DomainError) -> ClientError {
    invalid(&error.to_string())
}
fn io_error(error: std::io::Error) -> ClientError {
    ClientError::transport(format!("archive I/O: {error}"))
}
