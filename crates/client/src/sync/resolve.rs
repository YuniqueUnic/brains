//! Explicit conflict resolution using a selected side or a manually merged file.

use brains_api_types::Entry;

use crate::ClientResult;

use super::merge::load_live;
use super::{
    Disposition, FileState, PairSnapshot, Resolution, Snapshot, SyncEndpoint, SyncState, head_map,
    invalid, is_live, media, publish, reconcile_path, snapshot_head, snapshot_mutation,
};

pub fn resolve<E: SyncEndpoint>(
    first: &E,
    second: &E,
    state: &mut SyncState,
    path: &str,
    resolution: Resolution<'_>,
) -> ClientResult<()> {
    let endpoints = [first, second];
    let heads = [head_map(first)?, head_map(second)?];
    let current = [heads[0].get(path).cloned(), heads[1].get(path).cloned()];
    let base = state.files.get(path);
    let check = reconcile_path(endpoints, path, current.clone(), base, true)?;
    if !matches!(check.disposition, Disposition::Conflict(_)) {
        return Err(invalid(
            "path is no longer in conflict; run sync before resolving",
        ));
    }
    let mut resolved: [Option<Snapshot>; 2] = [None, None];
    match resolution {
        Resolution::Keep(index) if index < 2 => {
            let other = 1 - index;
            match current[index].as_ref() {
                Some(entry) if is_live(entry)? => {
                    let file = load_live(endpoints[index], path, entry)?;
                    resolved[index] = Some(file.snapshot.clone());
                    resolved[other] = publish(
                        endpoints[other],
                        path,
                        current[other].as_ref(),
                        &file.bytes,
                        media(&file.snapshot)?,
                        false,
                    )?;
                }
                entry => {
                    resolved[index] = snapshot_optional(endpoints[index], path, entry)?;
                    if let Some(target) = current[other]
                        .as_ref()
                        .filter(|target| target.state == "live")
                    {
                        let result = endpoints[other].trash(path, &target.revision)?;
                        resolved[other] = Some(snapshot_mutation(&result, None, None)?);
                    } else {
                        resolved[other] =
                            snapshot_optional(endpoints[other], path, current[other].as_ref())?;
                    }
                }
            }
        }
        Resolution::Keep(_) => return Err(invalid("resolution side must be local or remote")),
        Resolution::Merged { bytes, media_type } => {
            for index in 0..2 {
                resolved[index] = publish(
                    endpoints[index],
                    path,
                    current[index].as_ref(),
                    bytes,
                    media_type,
                    false,
                )?;
            }
        }
    }
    store_pair(state, path, resolved);
    Ok(())
}

fn snapshot_optional<E: SyncEndpoint>(
    endpoint: &E,
    path: &str,
    entry: Option<&Entry>,
) -> ClientResult<Option<Snapshot>> {
    let Some(entry) = entry else { return Ok(None) };
    if is_live(entry)? {
        let metadata = endpoint.revision_meta(path, &entry.revision)?;
        let content = metadata
            .content
            .ok_or_else(|| invalid("live revision has no content"))?;
        Ok(Some(Snapshot {
            revision: entry.revision.clone(),
            state: FileState::Live,
            sha256: Some(content.sha256),
            media_type: Some(content.media_type),
        }))
    } else {
        Ok(Some(snapshot_head(entry)?))
    }
}

fn store_pair(state: &mut SyncState, path: &str, sides: [Option<Snapshot>; 2]) {
    if sides.iter().all(Option::is_none) {
        state.files.remove(path);
    } else {
        state.files.insert(path.to_owned(), PairSnapshot { sides });
    }
}
