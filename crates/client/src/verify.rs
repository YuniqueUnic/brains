//! Read-only verification of current contents, independent of sync checkpoints.

use crate::endpoint::invalid;
use crate::endpoint::{FileEndpoint, read_verified};
use crate::{BrainClient, ClientResult, urlencode};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, serde::Serialize)]
pub struct VerifyReport {
    pub live_files: usize,
    pub non_live_paths: usize,
    pub mismatches: Vec<String>,
}

impl VerifyReport {
    pub fn success(&self) -> bool {
        self.mismatches.is_empty()
    }
}

pub fn verify_current(
    first: &dyn FileEndpoint,
    second: &dyn FileEndpoint,
) -> ClientResult<VerifyReport> {
    let before = [first.current_files()?, second.current_files()?];
    let maps = before.each_ref().map(|entries| {
        entries
            .iter()
            .map(|entry| (entry.path.as_str(), entry))
            .collect::<BTreeMap<_, _>>()
    });
    let paths: BTreeSet<_> = maps[0].keys().chain(maps[1].keys()).copied().collect();
    let mut report = VerifyReport::default();
    for path in paths {
        let live = maps
            .each_ref()
            .map(|map| map.get(path).copied().filter(|entry| entry.state == "live"));
        match live {
            [Some(a), Some(b)] => {
                let (_, a) = read_verified(first, path, &a.revision)?;
                let (_, b) = read_verified(second, path, &b.revision)?;
                report.live_files += 1;
                if a.sha256 != b.sha256
                    || a.size != b.size
                    || a.media_type != b.media_type
                    || a.executable != b.executable
                {
                    report
                        .mismatches
                        .push(format!("{path}: content or metadata differs"));
                }
            }
            [None, None] => report.non_live_paths += 1,
            _ => report
                .mismatches
                .push(format!("{path}: live on only one endpoint")),
        }
    }
    for (side, endpoint) in [first, second].into_iter().enumerate() {
        if endpoint.current_files()? != before[side] {
            report.mismatches.push(format!(
                "endpoint {} changed during verification; retry when writes have stopped",
                side + 1
            ));
        }
    }
    Ok(report)
}

/// An index rebuild can leave stale projections for absent heads. Compare
/// identities as well as checking the rebuild's own failure list.
pub fn verify_index(client: &BrainClient) -> ClientResult<()> {
    let heads = client.current_files()?;
    let mut index = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut query = String::from("source=index&include_deleted=true&limit=100");
        if let Some(cursor) = &cursor {
            query.push_str(&format!("&cursor={}", urlencode(cursor)));
        }
        let page = client.list_files(&query)?;
        index.extend(page.items);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let identities = |entries: &[brains_api_types::Entry]| {
        entries
            .iter()
            .map(|e| {
                (
                    e.path.clone(),
                    (e.revision.clone(), e.generation, e.state.clone()),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    if identities(&heads) != identities(&index) || heads != client.current_files()? {
        return Err(invalid(
            "index differs from authoritative heads or heads changed; profile was not switched",
        ));
    }
    Ok(())
}

/// Fully rebuild and compare the disposable index before accepting a target.
pub fn rebuild_index(client: &BrainClient) -> ClientResult<()> {
    let mut cursor = None;
    loop {
        let page = client.reindex(cursor.as_deref(), 5)?;
        if !page.failed.is_empty() {
            return Err(invalid(&format!(
                "index rebuild failed for {} paths",
                page.failed.len()
            )));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    verify_index(client)
}
