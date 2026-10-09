//! Per-profile-pair, per-file synchronization over the existing revision API.
//!
//! The engine knows nothing about HTTP or storage adapters. `SyncEndpoint`
//! is the narrow transport boundary; local FS/SQLite and cloud R2/D1 continue
//! to use the same server API and CAS semantics.

use std::collections::{BTreeMap, BTreeSet};

use brains_api_types::{Entry, MutationResult, RevisionDto};
use serde::{Deserialize, Serialize};

use crate::{BrainClient, ClientError, ClientResult, Precondition, urlencode};

mod merge;
mod resolve;

pub use resolve::resolve;

use merge::{both_sides, load_live, publish};

const STATE_FORMAT: &str = "brains-sync-state";
const STATE_VERSION: u32 = 1;

/// Operations required by the sync engine. Implementations must retain the
/// server's revision preconditions; writes are never unconditional.
pub trait SyncEndpoint {
    fn current_files(&self) -> ClientResult<Vec<Entry>>;
    fn revision_meta(&self, path: &str, revision: &str) -> ClientResult<RevisionDto>;
    fn revision_bytes(&self, path: &str, revision: &str) -> ClientResult<Vec<u8>>;
    fn put(
        &self,
        path: &str,
        bytes: &[u8],
        media_type: &str,
        precondition: Precondition,
    ) -> ClientResult<MutationResult>;
    fn trash(&self, path: &str, revision: &str) -> ClientResult<MutationResult>;
    fn restore(&self, path: &str, revision: &str, if_current: &str)
    -> ClientResult<MutationResult>;
}

impl SyncEndpoint for BrainClient {
    fn current_files(&self) -> ClientResult<Vec<Entry>> {
        let mut files = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut query = String::from("source=store&include_deleted=true&limit=100");
            if let Some(cursor) = &cursor {
                query.push_str("&cursor=");
                query.push_str(&urlencode(cursor));
            }
            let page = self.list_files(&query)?;
            files.extend(page.items);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return Ok(files),
            }
        }
    }

    fn revision_meta(&self, path: &str, revision: &str) -> ClientResult<RevisionDto> {
        BrainClient::revision_meta(self, path, revision)
    }

    fn revision_bytes(&self, path: &str, revision: &str) -> ClientResult<Vec<u8>> {
        BrainClient::revision_raw(self, path, revision)
    }

    fn put(
        &self,
        path: &str,
        bytes: &[u8],
        media_type: &str,
        precondition: Precondition,
    ) -> ClientResult<MutationResult> {
        self.put_content(path, bytes, media_type, precondition, None)
    }

    fn trash(&self, path: &str, revision: &str) -> ClientResult<MutationResult> {
        self.delete(path, revision, None)
    }

    fn restore(
        &self,
        path: &str,
        revision: &str,
        if_current: &str,
    ) -> ClientResult<MutationResult> {
        BrainClient::restore(self, path, revision, if_current, None)
    }
}

/// Durable metadata for one ordered endpoint pair. It contains no file bytes,
/// credentials, or revision history.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncState {
    format: String,
    version: u32,
    endpoints: [String; 2],
    files: BTreeMap<String, PairSnapshot>,
}

impl SyncState {
    pub fn new(endpoints: [String; 2]) -> Self {
        Self {
            format: STATE_FORMAT.to_owned(),
            version: STATE_VERSION,
            endpoints,
            files: BTreeMap::new(),
        }
    }

    pub fn validate(&self, endpoints: &[String; 2]) -> ClientResult<()> {
        if self.format != STATE_FORMAT || self.version != STATE_VERSION {
            return Err(invalid("unsupported sync state format or version"));
        }
        if &self.endpoints != endpoints {
            return Err(invalid("sync state belongs to a different endpoint pair"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PairSnapshot {
    sides: [Option<Snapshot>; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Snapshot {
    revision: String,
    state: FileState,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FileState {
    Live,
    Trashed,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SyncCounts {
    pub checked: usize,
    pub copied_first_to_second: usize,
    pub copied_second_to_first: usize,
    pub merged: usize,
    pub trashed: usize,
    pub unchanged: usize,
    pub skipped_purged: usize,
}

#[derive(Clone, Debug)]
pub struct SyncConflict {
    pub path: String,
    pub reason: ConflictReason,
    pub first_revision: Option<String>,
    pub second_revision: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum ConflictReason {
    FirstSyncDivergence,
    TextOverlap,
    Binary,
    DeletedAndModified,
    MissingBase,
    MediaTypeMismatch,
    PurgedAndModified,
}

impl std::fmt::Display for ConflictReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::FirstSyncDivergence => "different content before the first sync",
            Self::TextOverlap => "overlapping text edits",
            Self::Binary => "binary files changed on both sides",
            Self::DeletedAndModified => "one side deleted while the other changed",
            Self::MissingBase => "the common base revision is unavailable",
            Self::MediaTypeMismatch => "media types differ",
            Self::PurgedAndModified => "one side was permanently purged while the other changed",
        };
        f.write_str(text)
    }
}

pub struct SyncReport {
    pub state: SyncState,
    pub counts: SyncCounts,
    pub conflicts: Vec<SyncConflict>,
}

pub enum Resolution<'a> {
    Keep(usize),
    Merged {
        bytes: &'a [u8],
        media_type: &'a str,
    },
}

#[derive(Clone, Copy)]
enum Disposition {
    Unchanged,
    Copied(usize),
    Merged,
    Trashed,
    Purged,
    SkippedPurged,
    Conflict(ConflictReason),
}

struct Reconciled {
    pair: Option<PairSnapshot>,
    forget: bool,
    disposition: Disposition,
}

struct LoadedFile {
    snapshot: Snapshot,
    bytes: Vec<u8>,
}

pub fn synchronize<E: SyncEndpoint>(
    first: &E,
    second: &E,
    mut state: SyncState,
    dry_run: bool,
) -> ClientResult<SyncReport> {
    let endpoints = [first, second];
    let heads = [head_map(first)?, head_map(second)?];
    let mut paths = BTreeSet::new();
    paths.extend(heads[0].keys().cloned());
    paths.extend(heads[1].keys().cloned());
    paths.extend(state.files.keys().cloned());

    let mut counts = SyncCounts::default();
    let mut conflicts = Vec::new();
    for path in paths {
        counts.checked += 1;
        let current = [heads[0].get(&path).cloned(), heads[1].get(&path).cloned()];
        let revisions = [
            current[0].as_ref().map(|entry| entry.revision.clone()),
            current[1].as_ref().map(|entry| entry.revision.clone()),
        ];
        let base = state.files.get(&path).cloned();
        let result = reconcile_path(endpoints, &path, current, base.as_ref(), dry_run)?;
        match result.disposition {
            Disposition::Unchanged => counts.unchanged += 1,
            Disposition::Copied(0) => counts.copied_first_to_second += 1,
            Disposition::Copied(_) => counts.copied_second_to_first += 1,
            Disposition::Merged => counts.merged += 1,
            Disposition::Trashed => counts.trashed += 1,
            Disposition::Purged => {
                state.files.remove(&path);
                counts.unchanged += 1;
            }
            Disposition::SkippedPurged => counts.skipped_purged += 1,
            Disposition::Conflict(reason) => conflicts.push(SyncConflict {
                path: path.clone(),
                reason,
                first_revision: revisions[0].clone(),
                second_revision: revisions[1].clone(),
            }),
        }
        if let Some(pair) = result.pair {
            state.files.insert(path, pair);
        } else if result.forget {
            state.files.remove(&path);
        }
    }
    Ok(SyncReport {
        state,
        counts,
        conflicts,
    })
}

fn reconcile_path<E: SyncEndpoint>(
    endpoints: [&E; 2],
    path: &str,
    current: [Option<Entry>; 2],
    base: Option<&PairSnapshot>,
    dry_run: bool,
) -> ClientResult<Reconciled> {
    let Some(base) = base else {
        return first_sync(endpoints, path, current, dry_run);
    };
    let changed = [
        !same_head(current[0].as_ref(), base.sides[0].as_ref())?,
        !same_head(current[1].as_ref(), base.sides[1].as_ref())?,
    ];
    match changed {
        [false, false] => Ok(reconciled(None, Disposition::Unchanged)),
        [true, false] => one_sided(endpoints, path, current, base, 0, dry_run),
        [false, true] => one_sided(endpoints, path, current, base, 1, dry_run),
        [true, true] => both_sides(endpoints, path, current, base, dry_run),
    }
}

fn first_sync<E: SyncEndpoint>(
    endpoints: [&E; 2],
    path: &str,
    current: [Option<Entry>; 2],
    dry_run: bool,
) -> ClientResult<Reconciled> {
    match (&current[0], &current[1]) {
        (Some(a), None) if is_live(a)? => {
            let file = load_live(endpoints[0], path, a)?;
            let other = publish(
                endpoints[1],
                path,
                None,
                &file.bytes,
                media(&file.snapshot)?,
                dry_run,
            )?;
            Ok(with_pair(
                [Some(file.snapshot), other],
                Disposition::Copied(0),
            ))
        }
        (None, Some(b)) if is_live(b)? => {
            let file = load_live(endpoints[1], path, b)?;
            let other = publish(
                endpoints[0],
                path,
                None,
                &file.bytes,
                media(&file.snapshot)?,
                dry_run,
            )?;
            Ok(with_pair(
                [other, Some(file.snapshot)],
                Disposition::Copied(1),
            ))
        }
        (Some(a), Some(b)) if is_live(a)? && is_live(b)? => {
            let left = load_live(endpoints[0], path, a)?;
            let right = load_live(endpoints[1], path, b)?;
            if left.snapshot.sha256 == right.snapshot.sha256
                && left.snapshot.media_type == right.snapshot.media_type
            {
                Ok(with_pair(
                    [Some(left.snapshot), Some(right.snapshot)],
                    Disposition::Unchanged,
                ))
            } else {
                Ok(conflicted(ConflictReason::FirstSyncDivergence))
            }
        }
        (Some(a), Some(b)) if is_trashed(a)? && is_trashed(b)? => Ok(with_pair(
            [Some(snapshot_head(a)?), Some(snapshot_head(b)?)],
            Disposition::Unchanged,
        )),
        (Some(a), None) if is_trashed(a)? => Ok(with_pair(
            [Some(snapshot_head(a)?), None],
            Disposition::Unchanged,
        )),
        (None, Some(b)) if is_trashed(b)? => Ok(with_pair(
            [None, Some(snapshot_head(b)?)],
            Disposition::Unchanged,
        )),
        (None, None) => Ok(Reconciled {
            pair: None,
            forget: true,
            disposition: Disposition::Purged,
        }),
        _ => Ok(conflicted(ConflictReason::DeletedAndModified)),
    }
}

fn one_sided<E: SyncEndpoint>(
    endpoints: [&E; 2],
    path: &str,
    current: [Option<Entry>; 2],
    base: &PairSnapshot,
    source: usize,
    dry_run: bool,
) -> ClientResult<Reconciled> {
    let target = 1 - source;
    let Some(source_entry) = current[source].as_ref() else {
        if current[target].is_none() {
            return Ok(Reconciled {
                pair: None,
                forget: true,
                disposition: Disposition::Purged,
            });
        }
        if let Some(target_entry) = current[target].as_ref()
            && is_trashed(target_entry)?
        {
            let mut sides = [None, None];
            sides[target] = Some(snapshot_head(target_entry)?);
            return Ok(with_pair(sides, Disposition::SkippedPurged));
        }
        return Ok(reconciled(None, Disposition::SkippedPurged));
    };
    let Some(target_entry) = current[target].as_ref() else {
        if base.sides[target].is_some() {
            return Ok(reconciled(None, Disposition::SkippedPurged));
        }
        if is_live(source_entry)? {
            let source_file = load_live(endpoints[source], path, source_entry)?;
            let target_snapshot = publish(
                endpoints[target],
                path,
                None,
                &source_file.bytes,
                media(&source_file.snapshot)?,
                dry_run,
            )?;
            let mut sides = [None, None];
            sides[source] = Some(source_file.snapshot);
            sides[target] = target_snapshot;
            return Ok(with_pair(sides, Disposition::Copied(source)));
        }
        let mut sides = [None, None];
        sides[source] = Some(snapshot_head(source_entry)?);
        return Ok(with_pair(sides, Disposition::Unchanged));
    };

    if is_trashed(source_entry)? {
        if is_trashed(target_entry)? {
            let mut sides = [None, None];
            sides[source] = Some(snapshot_head(source_entry)?);
            sides[target] = Some(snapshot_head(target_entry)?);
            return Ok(with_pair(sides, Disposition::Unchanged));
        }
        if base.sides[target]
            .as_ref()
            .is_some_and(|snapshot| snapshot.state == FileState::Live)
        {
            let deleted = if dry_run {
                None
            } else {
                Some(snapshot_mutation(
                    &endpoints[target].trash(path, &target_entry.revision)?,
                    None,
                    None,
                )?)
            };
            let mut sides = [None, None];
            sides[source] = Some(snapshot_head(source_entry)?);
            sides[target] = deleted;
            return Ok(with_pair(sides, Disposition::Trashed));
        }
        return Ok(conflicted(ConflictReason::DeletedAndModified));
    }

    if is_trashed(target_entry)? {
        return Ok(conflicted(ConflictReason::DeletedAndModified));
    }
    let source_file = load_live(endpoints[source], path, source_entry)?;
    let target_base = base.sides[target].as_ref();
    if target_base.is_some_and(|snapshot| {
        snapshot.sha256 == source_file.snapshot.sha256
            && snapshot.media_type == source_file.snapshot.media_type
    }) {
        let mut sides = [None, None];
        sides[source] = Some(source_file.snapshot);
        sides[target] = target_base.cloned();
        return Ok(with_pair(sides, Disposition::Unchanged));
    }
    let target_snapshot = publish(
        endpoints[target],
        path,
        Some(target_entry),
        &source_file.bytes,
        media(&source_file.snapshot)?,
        dry_run,
    )?;
    let mut sides = [None, None];
    sides[source] = Some(source_file.snapshot);
    sides[target] = target_snapshot;
    Ok(with_pair(sides, Disposition::Copied(source)))
}

fn head_map<E: SyncEndpoint>(endpoint: &E) -> ClientResult<BTreeMap<String, Entry>> {
    endpoint
        .current_files()?
        .into_iter()
        .map(|entry| Ok((entry.path.clone(), validate_entry(entry)?)))
        .collect()
}

fn validate_entry(entry: Entry) -> ClientResult<Entry> {
    if matches!(entry.state.as_str(), "live" | "trashed") {
        Ok(entry)
    } else {
        Err(invalid(&format!("unknown file state `{}`", entry.state)))
    }
}

fn same_head(entry: Option<&Entry>, snapshot: Option<&Snapshot>) -> ClientResult<bool> {
    match (entry, snapshot) {
        (None, None) => Ok(true),
        (Some(entry), Some(snapshot)) => {
            Ok(entry.revision == snapshot.revision && entry.state == state_name(snapshot.state))
        }
        _ => Ok(false),
    }
}

fn snapshot_head(entry: &Entry) -> ClientResult<Snapshot> {
    Ok(Snapshot {
        revision: entry.revision.clone(),
        state: if is_live(entry)? {
            FileState::Live
        } else {
            FileState::Trashed
        },
        sha256: entry.sha256.clone(),
        media_type: entry.media_type.clone(),
    })
}

fn snapshot_mutation(
    result: &MutationResult,
    sha256: Option<&str>,
    media_type: Option<&str>,
) -> ClientResult<Snapshot> {
    Ok(Snapshot {
        revision: result.entry.revision.clone(),
        state: if is_live(&result.entry)? {
            FileState::Live
        } else {
            FileState::Trashed
        },
        sha256: sha256
            .map(str::to_owned)
            .or_else(|| result.entry.sha256.clone()),
        media_type: media_type
            .map(str::to_owned)
            .or_else(|| result.entry.media_type.clone()),
    })
}

fn is_live(entry: &Entry) -> ClientResult<bool> {
    match entry.state.as_str() {
        "live" => Ok(true),
        "trashed" => Ok(false),
        state => Err(invalid(&format!("unknown file state `{state}`"))),
    }
}

fn is_trashed(entry: &Entry) -> ClientResult<bool> {
    Ok(!is_live(entry)?)
}

fn state_name(state: FileState) -> &'static str {
    match state {
        FileState::Live => "live",
        FileState::Trashed => "trashed",
    }
}

fn media(snapshot: &Snapshot) -> ClientResult<&str> {
    snapshot
        .media_type
        .as_deref()
        .ok_or_else(|| invalid("live snapshot is missing media type"))
}

fn with_pair(sides: [Option<Snapshot>; 2], disposition: Disposition) -> Reconciled {
    Reconciled {
        pair: Some(PairSnapshot { sides }),
        forget: false,
        disposition,
    }
}

fn reconciled(pair: Option<PairSnapshot>, disposition: Disposition) -> Reconciled {
    Reconciled {
        pair,
        forget: false,
        disposition,
    }
}

fn conflicted(reason: ConflictReason) -> Reconciled {
    reconciled(None, Disposition::Conflict(reason))
}

fn invalid(message: &str) -> ClientError {
    ClientError {
        status: 422,
        code: Some("SYNC_STATE_INVALID".into()),
        message: message.to_owned(),
        current_revision: None,
    }
}
