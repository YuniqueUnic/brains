//! Listing, search, and link query commands.

use crate::cli::LsArgs;
use brains_client::{ClientError, ClientResult, Library};

/// `--stale`/`--fresh` share one dimension: at most one may be requested.
fn stale_flag(stale: bool, fresh: bool) -> Option<bool> {
    match (stale, fresh) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

pub(crate) fn ls(client: &dyn Library, args: LsArgs) -> ClientResult<i32> {
    let LsArgs {
        mut prefix,
        json,
        deleted,
        source,
        tag,
        kind,
        sort,
        status,
        type_,
        trust,
        stale,
        fresh,
    } = args;
    // `bsx ls /` reads as "the whole vault" (§6.1): paths are vault-relative
    // and never start with a slash.
    if prefix == "/" {
        prefix.clear();
    }
    let mut cursor: Option<String> = None;
    loop {
        let query = brains_application::listing::ListQuery {
            source: source
                .parse()
                .map_err(|e: brains_domain::DomainError| ClientError {
                    status: 400,
                    code: Some(e.code().into()),
                    message: e.to_string(),
                    current_revision: None,
                })?,
            prefix: (!prefix.is_empty()).then(|| prefix.clone()),
            cursor: cursor.clone(),
            tags: tag.clone(),
            kind: kind.as_deref().and_then(brains_domain::FileKind::parse),
            status: status.clone(),
            type_: type_.clone(),
            trust: trust.clone(),
            stale: stale_flag(stale, fresh),
            sort: if sort == "updated_desc" {
                brains_ports::ListOrder::UpdatedDesc
            } else {
                brains_ports::ListOrder::Path
            },
            include_deleted: deleted,
            limit: 50,
        };
        let page = client.list_files(&query)?;
        for entry in &page.items {
            if json {
                println!(
                    "{}",
                    serde_json::to_string(entry)
                        .map_err(|e| ClientError::transport(e.to_string()))?
                );
            } else {
                let state = if entry.state == "trashed" {
                    "[trashed]"
                } else {
                    ""
                };
                let title = entry.title.clone().unwrap_or_default();
                println!("{}\t{}\t{state}{title}", entry.path, entry.revision);
            }
        }
        match page.next_cursor.clone() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn search(
    client: &dyn Library,
    query: &str,
    mode: &str,
    prefix: Option<String>,
    tag: Vec<String>,
    status: Option<String>,
    type_: Option<String>,
    trust: Option<String>,
    stale: bool,
    fresh: bool,
    json: bool,
    max_pages: Option<usize>,
) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    let mut pages = 0usize;
    let mut total_hits = 0usize;
    let mut complete;
    loop {
        let q = brains_application::search::SearchQuery {
            q: query.to_owned(),
            mode: match mode {
                "literal" => brains_application::search::SearchMode::Literal,
                "fts" => brains_application::search::SearchMode::Fts,
                "hybrid" => brains_application::search::SearchMode::Hybrid,
                _ => {
                    return Err(ClientError {
                        status: 400,
                        code: Some("INVALID_ARGUMENT".into()),
                        message: format!("unknown search mode `{mode}`"),
                        current_revision: None,
                    });
                }
            },
            prefix: prefix.clone(),
            tags: tag.clone(),
            status: status.clone(),
            type_: type_.clone(),
            trust: trust.clone(),
            stale: stale_flag(stale, fresh),
            cursor: cursor.clone(),
            scan_limit: brains_domain::Limits::default().scan_window_max,
        };
        let page = client.search(&q)?;
        pages += 1;
        total_hits += page.items.len();
        for hit in &page.items {
            if json {
                println!(
                    "{}",
                    serde_json::to_string(hit)
                        .map_err(|e| ClientError::transport(e.to_string()))?
                );
            } else {
                let snippet: String = hit.snippet.chars().take(120).collect();
                let retrievers = if hit.retrieved_by.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\t{}",
                        hit.retrieved_by
                            .iter()
                            .map(|r| format!("{}#{}", r.retriever, r.rank))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                };
                println!("{}\t{}{}", hit.path, snippet.replace('\n', " "), retrievers);
            }
        }
        complete = page.complete;
        if !page.unavailable.is_empty() {
            eprintln!(
                "bsx: hybrid retrievers unavailable: {}",
                page.unavailable.join(", ")
            );
        }
        match page.next_cursor.clone() {
            Some(next) if max_pages.is_none_or(|max| pages < max) => cursor = Some(next),
            _ => break,
        }
    }
    if !complete {
        eprintln!("bsx: search incomplete after {pages} pages; results may be partial");
        return Ok(7);
    }
    Ok(if total_hits == 0 { 1 } else { 0 })
}

pub(crate) fn links(
    client: &dyn Library,
    path: &str,
    backlinks: bool,
    json: bool,
) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    loop {
        let page = client.links(path, backlinks, cursor.as_deref())?;
        for item in &page.items {
            if json {
                println!(
                    "{}",
                    serde_json::to_string(item)
                        .map_err(|e| ClientError::transport(e.to_string()))?
                );
            } else if backlinks {
                println!("{}\t{}", item.source, item.kind);
            } else {
                println!("{}\t{}", item.target, item.kind);
            }
        }
        match page.next_cursor.clone() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(0)
}
