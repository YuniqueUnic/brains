//! Working-tree export over HTTP primitives (design §11.1, §14.4): paginate
//! authoritative heads, download each live file, verify digests, apply the
//! portability checks, publish atomically with a manifest. Per-file
//! consistency — never a whole-vault snapshot claim.

use serde::Serialize;
use sha2::{Digest, Sha256};

use brains_domain::path::check_portable_segment;

use crate::{BrainClient, ClientError, ClientResult, urlencode};

#[derive(Serialize)]
struct ManifestEntry {
    path: String,
    revision: String,
    generation: i64,
    sha256: String,
    size: i64,
    executable_restored: bool,
}

#[derive(Serialize)]
struct ManifestTombstone {
    path: String,
    revision: String,
}

#[derive(Serialize)]
struct Manifest {
    format: &'static str,
    format_version: u32,
    vault: String,
    consistency: &'static str,
    started_at: String,
    finished_at: String,
    history_verified: bool,
    entries: Vec<ManifestEntry>,
    tombstones: Vec<ManifestTombstone>,
}

/// Export the current working tree to a NEW directory `output`.
/// Two phases (design §14.4): enumerate the authoritative listing to the
/// end FIRST, then download each entry's bytes from its recorded immutable
/// revision — a concurrent writer mid-export can neither corrupt a file nor
/// desynchronize the manifest. Fails loudly on collision or portability
/// problems instead of dropping a file (design §6.2, U16).
pub fn export_working_tree(
    client: &BrainClient,
    output: &std::path::Path,
    vault: &str,
) -> ClientResult<String> {
    if output.exists() {
        return Err(ClientError {
            status: 400,
            code: Some("INVALID_ARGUMENT".into()),
            message: format!("output directory already exists: {}", output.display()),
            current_revision: None,
        });
    }
    let parent = output.parent().ok_or_else(|| ClientError {
        status: 400,
        code: Some("INVALID_ARGUMENT".into()),
        message: "output needs a parent directory".into(),
        current_revision: None,
    })?;
    let started_at = now_rfc3339();
    let staging = parent.join(format!(
        ".{}-export-{}",
        output
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "vault".into()),
        brains_domain::HexId::generate()
    ));
    std::fs::create_dir_all(&staging).map_err(io_err)?;
    // Any failure below leaves no staging litter behind.
    match export_into_staging(client, &staging, vault, started_at) {
        Ok(manifest) => {
            // Same-parent publish; the output must still not exist.
            if output.exists() {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(ClientError {
                    status: 400,
                    code: Some("INVALID_ARGUMENT".into()),
                    message: "output appeared during export".into(),
                    current_revision: None,
                });
            }
            std::fs::rename(&staging, output).map_err(|e| {
                let _ = std::fs::remove_dir_all(&staging);
                ClientError::transport(format!("publish failed: {e}"))
            })?;
            Ok(serde_json::json!({
                "output": output.display().to_string(),
                "files": manifest.entries.len(),
                "tombstones": manifest.tombstones.len(),
            })
            .to_string())
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

fn export_into_staging(
    client: &BrainClient,
    staging: &std::path::Path,
    vault: &str,
    started_at: String,
) -> ClientResult<Manifest> {
    // Phase 1: enumerate the authoritative listing to the end.
    let mut listed: Vec<brains_api_types::Entry> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut query = String::from("source=store&include_deleted=true&limit=100");
        if let Some(cursor) = &cursor {
            query.push_str(&format!("&cursor={}", urlencode(cursor)));
        }
        let page = client.list_files(&query)?;
        listed.extend(page.items);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    check_portability(&listed.iter().map(|e| e.path.clone()).collect::<Vec<_>>())?;

    // Phase 2: download each entry's recorded immutable revision.
    let mut entries: Vec<ManifestEntry> = Vec::new();
    let mut tombstones: Vec<ManifestTombstone> = Vec::new();
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
        let meta = client.revision_meta(&entry.path, &entry.revision)?;
        let expected = meta.content.as_ref().map(|c| c.sha256.clone());
        let bytes = client.revision_raw(&entry.path, &entry.revision)?;
        let actual = hex::encode(Sha256::digest(&bytes));
        if let Some(expected) = &expected
            && actual != *expected
        {
            return Err(ClientError {
                status: 422,
                code: Some("CHECKSUM_MISMATCH".into()),
                message: format!(
                    "revision bytes do not match server digest for {} @ {}",
                    entry.path, entry.revision
                ),
                current_revision: None,
            });
        }
        write_entry_file(staging, &entry.path, &bytes)?;
        entries.push(ManifestEntry {
            path: entry.path.clone(),
            revision: entry.revision.clone(),
            generation: entry.generation,
            sha256: expected.unwrap_or(actual),
            size: bytes.len() as i64,
            executable_restored: false,
        });
    }

    let manifest = Manifest {
        format: "brains-working-tree",
        format_version: 1,
        vault: vault.to_owned(),
        consistency: "per_file",
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

fn write_entry_file(root: &std::path::Path, logical: &str, bytes: &[u8]) -> ClientResult<()> {
    let target = root.join(logical);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io_err)?;
    }
    std::fs::write(&target, bytes).map_err(io_err)
}

/// The checks export_raw.py also applies: reserved names, portable segments,
/// case-fold collisions, file/dir conflicts.
fn check_portability(paths: &[String]) -> ClientResult<()> {
    let mut folded_dirs: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut folded_files: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for path in paths {
        let parts: Vec<&str> = path.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            check_portable_segment(part).map_err(|reason| ClientError {
                status: 422,
                code: Some("INVALID_FORMAT".into()),
                message: format!("path `{path}` is not portable: {reason}"),
                current_revision: None,
            })?;
            let original = parts[..=index].join("/");
            let folded = original.to_lowercase();
            if index + 1 < parts.len() {
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
