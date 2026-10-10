//! Three-way merge and revision transfer operations.

use brains_api_types::Entry;
use sha2::{Digest, Sha256};

use crate::{ClientResult, Precondition};

use super::{
    ConflictReason, Disposition, FileEndpoint, FileState, LoadedFile, PairSnapshot, Reconciled,
    Snapshot, conflicted, invalid, is_live, is_trashed, media, snapshot_head, snapshot_mutation,
    with_pair,
};

pub(super) fn both_sides(
    endpoints: [&dyn FileEndpoint; 2],
    path: &str,
    current: [Option<Entry>; 2],
    base: &PairSnapshot,
    dry_run: bool,
) -> ClientResult<Reconciled> {
    let (Some(left), Some(right)) = (&current[0], &current[1]) else {
        return match (&current[0], &current[1]) {
            (None, None) => Ok(Reconciled {
                pair: None,
                forget: true,
                disposition: Disposition::Purged,
            }),
            (None, Some(right)) if is_trashed(right)? => Ok(with_pair(
                [None, Some(snapshot_head(right)?)],
                Disposition::SkippedPurged,
            )),
            (Some(left), None) if is_trashed(left)? => Ok(with_pair(
                [Some(snapshot_head(left)?), None],
                Disposition::SkippedPurged,
            )),
            _ => Ok(conflicted(ConflictReason::PurgedAndModified)),
        };
    };
    if is_trashed(left)? && is_trashed(right)? {
        return Ok(with_pair(
            [Some(snapshot_head(left)?), Some(snapshot_head(right)?)],
            Disposition::Unchanged,
        ));
    }
    if is_trashed(left)? || is_trashed(right)? {
        return Ok(conflicted(ConflictReason::DeletedAndModified));
    }
    let left_file = load_live(endpoints[0], path, left)?;
    let right_file = load_live(endpoints[1], path, right)?;
    if left_file.snapshot.sha256 == right_file.snapshot.sha256
        && left_file.snapshot.media_type == right_file.snapshot.media_type
        && left_file.snapshot.executable == right_file.snapshot.executable
    {
        return Ok(with_pair(
            [Some(left_file.snapshot), Some(right_file.snapshot)],
            Disposition::Unchanged,
        ));
    }
    if media(&left_file.snapshot)? != media(&right_file.snapshot)? {
        return Ok(conflicted(ConflictReason::MediaTypeMismatch));
    }
    if left_file.snapshot.executable != right_file.snapshot.executable {
        return Ok(conflicted(ConflictReason::ExecutableMismatch));
    }
    let executable = left_file.snapshot.executable;
    let media_type = media(&left_file.snapshot)?;
    if !brains_domain::is_text_media(media_type) {
        return Ok(conflicted(ConflictReason::Binary));
    }
    let Some(base_file) = read_base(endpoints, path, base)? else {
        return Ok(conflicted(ConflictReason::MissingBase));
    };
    let (Ok(ancestor), Ok(ours), Ok(theirs)) = (
        std::str::from_utf8(&base_file.bytes),
        std::str::from_utf8(&left_file.bytes),
        std::str::from_utf8(&right_file.bytes),
    ) else {
        return Ok(conflicted(ConflictReason::Binary));
    };
    let Ok(merged) = diffy::merge(ancestor, ours, theirs) else {
        return Ok(conflicted(ConflictReason::TextOverlap));
    };
    let bytes = merged.into_bytes();
    let left_next = if bytes == left_file.bytes {
        Some(left_file.snapshot.clone())
    } else {
        publish(
            endpoints[0],
            path,
            Some(left),
            &bytes,
            media_type,
            executable,
            dry_run,
        )?
    };
    let right_next = if bytes == right_file.bytes {
        Some(right_file.snapshot.clone())
    } else {
        publish(
            endpoints[1],
            path,
            Some(right),
            &bytes,
            media_type,
            executable,
            dry_run,
        )?
    };
    Ok(with_pair([left_next, right_next], Disposition::Merged))
}

fn read_base(
    endpoints: [&dyn FileEndpoint; 2],
    path: &str,
    base: &PairSnapshot,
) -> ClientResult<Option<LoadedFile>> {
    for (index, endpoint) in endpoints.iter().enumerate() {
        let Some(snapshot) = base.sides[index]
            .as_ref()
            .filter(|snapshot| snapshot.state == FileState::Live)
        else {
            continue;
        };
        match load_revision(*endpoint, path, snapshot) {
            Ok(file)
                if file.snapshot.sha256 == snapshot.sha256
                    && file.snapshot.media_type == snapshot.media_type
                    && file.snapshot.executable == snapshot.executable =>
            {
                return Ok(Some(file));
            }
            Ok(_) => continue,
            Err(error) if matches!(error.status, 404 | 410) => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

pub(super) fn load_live(
    endpoint: &dyn FileEndpoint,
    path: &str,
    entry: &Entry,
) -> ClientResult<LoadedFile> {
    load_revision(endpoint, path, &snapshot_head(entry)?)
}

fn load_revision(
    endpoint: &dyn FileEndpoint,
    path: &str,
    snapshot: &Snapshot,
) -> ClientResult<LoadedFile> {
    let (bytes, content) = crate::endpoint::read_verified(endpoint, path, &snapshot.revision)?;
    Ok(LoadedFile {
        snapshot: Snapshot {
            revision: snapshot.revision.clone(),
            state: FileState::Live,
            sha256: Some(content.sha256),
            media_type: Some(content.media_type),
            executable: content.executable,
        },
        bytes,
    })
}

pub(super) fn publish(
    endpoint: &dyn FileEndpoint,
    path: &str,
    target: Option<&Entry>,
    bytes: &[u8],
    media_type: &str,
    executable: bool,
    dry_run: bool,
) -> ClientResult<Option<Snapshot>> {
    let digest = hex::encode(Sha256::digest(bytes));
    let expected = match target {
        None => {
            return if dry_run {
                Ok(None)
            } else {
                let result =
                    endpoint.put(path, bytes, media_type, executable, Precondition::Create)?;
                Ok(Some(snapshot_mutation(
                    &result,
                    Some(&digest),
                    Some(media_type),
                    executable,
                )?))
            };
        }
        Some(entry) if is_live(entry)? => entry.revision.clone(),
        Some(entry) if is_trashed(entry)? => {
            let metadata = endpoint.revision_meta(path, &entry.revision)?;
            let parent = metadata
                .parent
                .ok_or_else(|| invalid("tombstone has no live parent"))?;
            if dry_run {
                return Ok(None);
            }
            let restored = endpoint.restore(path, &parent, &entry.revision)?;
            snapshot_mutation(&restored, None, None, executable)?;
            restored.entry.revision
        }
        Some(_) => return Err(invalid("unsupported file state")),
    };
    if let Some(entry) = target
        && is_live(entry)?
    {
        let metadata = endpoint.revision_meta(path, &entry.revision)?;
        if metadata.content.as_ref().is_some_and(|content| {
            content.sha256 == digest
                && content.media_type == media_type
                && content.executable == executable
        }) {
            return Ok(Some(Snapshot {
                revision: entry.revision.clone(),
                state: FileState::Live,
                sha256: Some(digest),
                media_type: Some(media_type.to_owned()),
                executable,
            }));
        }
    }
    if dry_run {
        return Ok(None);
    }
    let result = endpoint.put(
        path,
        bytes,
        media_type,
        executable,
        Precondition::IfRevision(expected),
    )?;
    Ok(Some(snapshot_mutation(
        &result,
        Some(&digest),
        Some(media_type),
        executable,
    )?))
}
