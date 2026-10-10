//! Listing, search, and link query commands.

use crate::cli::LsArgs;
use brains_client::{BrainClient, ClientError, ClientResult, urlencode};

pub(crate) fn ls(client: &BrainClient, args: LsArgs) -> ClientResult<i32> {
    let LsArgs {
        mut prefix,
        json,
        deleted,
        source,
        tag,
        kind,
        sort,
    } = args;
    // `bsx ls /` reads as "the whole vault" (§6.1): paths are vault-relative
    // and never start with a slash.
    if prefix == "/" {
        prefix.clear();
    }
    let mut cursor: Option<String> = None;
    loop {
        let mut query = format!("source={source}&limit=50");
        if !prefix.is_empty() {
            query.push_str(&format!("&prefix={}", urlencode(&prefix)));
        }
        if deleted {
            query.push_str("&include_deleted=true");
        }
        if let Some(tag) = &tag {
            query.push_str(&format!("&tag={}", urlencode(tag)));
        }
        if let Some(kind) = &kind {
            query.push_str(&format!("&kind={kind}"));
        }
        if sort != "path" {
            query.push_str(&format!("&sort={sort}"));
        }
        if let Some(cursor) = &cursor {
            query.push_str(&format!("&cursor={}", urlencode(cursor)));
        }
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

pub(crate) fn search(
    client: &BrainClient,
    query: &str,
    mode: &str,
    prefix: Option<String>,
    tag: Option<String>,
    json: bool,
    max_pages: Option<usize>,
) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    let mut pages = 0usize;
    let mut total_hits = 0usize;
    let mut complete;
    loop {
        let mut q = format!("q={}&mode={mode}", urlencode(query));
        if let Some(prefix) = &prefix {
            q.push_str(&format!("&prefix={}", urlencode(prefix)));
        }
        if let Some(tag) = &tag {
            q.push_str(&format!("&tag={}", urlencode(tag)));
        }
        if let Some(cursor) = &cursor {
            q.push_str(&format!("&cursor={}", urlencode(cursor)));
        }
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
                println!("{}\t{}", hit.path, snippet.replace('\n', " "));
            }
        }
        complete = page.complete;
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
    client: &BrainClient,
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
