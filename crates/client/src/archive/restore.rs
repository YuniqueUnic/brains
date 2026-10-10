use super::*;
use std::collections::BTreeMap;
use std::path::Path;

pub fn import_archive(
    endpoint: &dyn ArchiveEndpoint,
    root: &Path,
    dry_run: bool,
) -> ClientResult<String> {
    let archive = load_archive(root)?;
    let target = endpoint.archive_target()?;
    let vault = target.vault;
    for revision in archive.files.iter().flat_map(|file| &file.revisions) {
        if let Some(content) = &revision.content
            && content.size as u64 > target.max_blob_bytes as u64
        {
            return Err(invalid(&format!(
                "{} @ {} exceeds target attachment limit of {} bytes",
                revision.path, revision.revision, target.max_blob_bytes
            )));
        }
    }
    // Complete all-path preflight before publishing even the first revision.
    let before = endpoint.current_files()?;
    let source: BTreeMap<_, _> = archive
        .files
        .iter()
        .map(|f| (f.head.path.as_str(), f))
        .collect();
    let mut prefixes = BTreeMap::new();
    for entry in &before {
        let file = source.get(entry.path.as_str()).ok_or_else(|| {
            invalid(&format!(
                "target has a path outside this archive: {}",
                entry.path
            ))
        })?;
        let prefix = file
            .revisions
            .iter()
            .position(|r| r.revision.as_str() == entry.revision)
            .ok_or_else(|| invalid(&format!("target has different history: {}", entry.path)))?
            + 1;
        if !matches_entry(
            &file.revisions[prefix - 1].head().map_err(domain_error)?,
            entry,
        ) {
            return Err(invalid("target HEAD differs from archive prefix"));
        }
        verify_revisions(endpoint, &file.revisions[..prefix], &vault)?;
        prefixes.insert(entry.path.as_str(), prefix);
    }
    if before != endpoint.current_files()? {
        return Err(invalid("target changed during preflight"));
    }
    let revisions: usize = archive.files.iter().map(|f| f.revisions.len()).sum();
    if dry_run {
        return Ok(serde_json::json!({"dry_run": true, "paths": archive.files.len(), "revisions": revisions, "local_archive_verified": true, "target_prefixes_verified": true}).to_string());
    }
    let mut completed_paths = 0;
    let mut published = 0;
    for file in &archive.files {
        let prefix = prefixes.get(file.head.path.as_str()).copied().unwrap_or(0);
        for revision in &file.revisions[prefix..] {
            let outcome = (|| -> ClientResult<()> {
                let expected = revision.in_vault(&vault).map_err(domain_error)?;
                if endpoint.store_revision(revision)? != expected {
                    return Err(invalid("stored revision differs from archive"));
                }
                if let Some(content) = &revision.content {
                    let bytes = read_regular(root, &blob_path(root, &content.key))?;
                    verify_bytes(content, &bytes)?;
                    endpoint.store_blob(revision, &bytes)?;
                }
                if endpoint.publish_head(revision)? != revision.head().map_err(domain_error)? {
                    return Err(invalid("published HEAD differs from archive"));
                }
                Ok(())
            })();
            if let Err(mut error) = outcome {
                error.message = format!(
                    "restore stopped at {} @ {} after {completed_paths} paths / {published} published revisions; target contains a partial prefix, rerun the same archive: {}",
                    file.head.path, revision.revision, error.message
                );
                return Err(error);
            }
            published += 1;
        }
        completed_paths += 1;
    }
    // Acceptance reads every immutable object back, then rebuilds the index.
    // An error is never converted into a successful migration report.
    let accepted = (|| -> ClientResult<()> {
        for file in &archive.files {
            verify_revisions(endpoint, &file.revisions, &vault)?;
        }
        verify_heads(endpoint, &archive)?;
        endpoint.rebuild_index()?;
        verify_heads(endpoint, &archive)
    })();
    accepted.map_err(|mut error| {
        error.message = format!("all paths replayed but final archive verification failed; do not switch this target into daily use: {}", error.message);
        error
    })?;
    Ok(serde_json::json!({"paths": completed_paths, "revisions": revisions, "published": published, "history_verified": true, "index_verified": true}).to_string())
}

fn verify_revisions(
    endpoint: &dyn ArchiveEndpoint,
    revisions: &[Revision],
    vault: &str,
) -> ClientResult<()> {
    for revision in revisions {
        let expected = revision.in_vault(vault).map_err(domain_error)?;
        if endpoint.archive_revision(&revision.path, revision.revision.as_str())? != expected {
            return Err(invalid(&format!(
                "immutable revision differs: {} @ {}",
                revision.path, revision.revision
            )));
        }
        if let Some(content) = &revision.content {
            verify_bytes(
                content,
                &endpoint.revision_bytes(&revision.path, revision.revision.as_str())?,
            )?;
        }
    }
    Ok(())
}

fn verify_heads(endpoint: &dyn ArchiveEndpoint, archive: &Archive) -> ClientResult<()> {
    let heads = endpoint.current_files()?;
    let files: BTreeMap<_, _> = archive
        .files
        .iter()
        .map(|f| (f.head.path.as_str(), &f.head))
        .collect();
    if heads.len() != files.len()
        || heads.iter().any(|e| {
            !files
                .get(e.path.as_str())
                .is_some_and(|head| matches_entry(head, e))
        })
    {
        return Err(invalid("target HEADs do not match the complete archive"));
    }
    Ok(())
}
