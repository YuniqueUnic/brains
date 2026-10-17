//! `bsx checkout|status|pull|push` (issue #21): a tracked working copy for
//! editing notes from an editor or an agent's working directory, using only
//! existing APIs. `--dry-run` everywhere; conflicts are printed, never forced.

use std::path::Path;

use brains_client::working_copy;
use brains_client::{ClientError, ClientResult, Library};

fn describe(change: Option<working_copy::SideChange>) -> &'static str {
    match change {
        None => "",
        Some(working_copy::SideChange::Modified) => "modified",
        Some(working_copy::SideChange::Added) => "added",
        Some(working_copy::SideChange::Deleted) => "deleted",
    }
}

pub(crate) fn checkout(
    client: &dyn Library,
    directory: &Path,
    prefix: Option<&str>,
) -> ClientResult<i32> {
    let written = working_copy::checkout(client, directory, prefix)?;
    println!("checked out {written} files into {}", directory.display());
    Ok(0)
}

pub(crate) fn status(client: &dyn Library, directory: &Path, json: bool) -> ClientResult<i32> {
    let report = working_copy::status(client, directory)?;
    if json {
        serde_json::to_string_pretty(&report)
            .map_err(|e| ClientError::transport(e.to_string()))?
            .lines()
            .for_each(|line| println!("{line}"));
        return Ok(0);
    }
    for row in &report.rows {
        let marker = if row.local.is_some() && row.store.is_some() {
            "conflict"
        } else if row.local.is_some() {
            "local"
        } else {
            "store"
        };
        println!(
            "{}\t{}\t{}\t{}",
            marker,
            describe(row.local),
            describe(row.store),
            row.path
        );
    }
    eprintln!(
        "{} tracked, {} changed, {} clean",
        report.rows.len(),
        report
            .rows
            .iter()
            .filter(|row| row.local.is_some() && row.store.is_none())
            .count(),
        report.clean
    );
    Ok(0)
}

pub(crate) fn pull(
    client: &dyn Library,
    directory: &Path,
    dry_run: bool,
    json: bool,
) -> ClientResult<i32> {
    let report = working_copy::pull(client, directory, dry_run)?;
    if json {
        serde_json::to_string_pretty(&report)
            .map_err(|e| ClientError::transport(e.to_string()))?
            .lines()
            .for_each(|line| println!("{line}"));
        return Ok(if report.conflicts.is_empty() { 0 } else { 11 });
    }
    println!(
        "pull {}: {} updated, {} merged, {} removed, {} conflicts",
        if dry_run { "planned" } else { "done" },
        report.updated,
        report.merged,
        report.removed,
        report.conflicts.len()
    );
    for path in &report.conflicts {
        eprintln!("conflict\t{path}");
    }
    Ok(if report.conflicts.is_empty() { 0 } else { 11 })
}

pub(crate) fn push(
    client: &dyn Library,
    directory: &Path,
    dry_run: bool,
    json: bool,
) -> ClientResult<i32> {
    let report = working_copy::push(client, directory, dry_run)?;
    if json {
        serde_json::to_string_pretty(&report)
            .map_err(|e| ClientError::transport(e.to_string()))?
            .lines()
            .for_each(|line| println!("{line}"));
        return Ok(if report.conflicts.is_empty() { 0 } else { 11 });
    }
    println!(
        "push {}: {} written, {} trashed, {} conflicts",
        if dry_run { "planned" } else { "done" },
        report.written,
        report.trashed,
        report.conflicts.len()
    );
    for path in &report.conflicts {
        eprintln!("conflict\t{path}");
    }
    Ok(if report.conflicts.is_empty() { 0 } else { 11 })
}
