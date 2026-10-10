//! Index projection of committed revisions (design §10.1). The body is the
//! raw UTF-8 text including frontmatter — grep-like semantics. Binary and
//! oversized content is stored but never indexed.

use brains_domain::{FileKind, Revision, VaultPath, detect_kind};
use brains_ports::{FileProjection, PARSER_VERSION};

use brains_format::{extract_links, extract_title, split_frontmatter};

pub fn build_projection(
    vault: &str,
    revision: &Revision,
    content: Option<&[u8]>,
    deleted: bool,
    max_text_bytes: usize,
) -> FileProjection {
    let path = VaultPath::trusted(revision.path.clone());
    let kind = match content {
        Some(bytes) => detect_kind(revision.path.as_str(), bytes),
        None => detect_kind(revision.path.as_str(), b""),
    };
    let mut projection = FileProjection {
        path: revision.path.clone(),
        revision: revision.revision.as_str().to_owned(),
        generation: revision.generation,
        deleted: deleted || revision.action == brains_domain::RevisionAction::Trash,
        kind,
        title: String::new(),
        body: String::new(),
        size: revision.content.as_ref().map_or(0, |c| c.size),
        sha256: revision.content.as_ref().map(|c| c.sha256.clone()),
        media_type: revision.content.as_ref().map_or_else(
            || brains_domain::media_type_for(revision.path.as_str()).to_owned(),
            |c| c.media_type.clone(),
        ),
        tags_json: "[]".into(),
        links_json: "[]".into(),
        parser_version: PARSER_VERSION,
        updated_at: revision.created_at.clone(),
    };

    let Some(bytes) = content else {
        return projection;
    };
    if !kind.searchable() || projection.deleted {
        return projection;
    }
    if bytes.len() > max_text_bytes {
        // Declared behavior for oversized text: stored, but never indexed —
        // the body/FTS rows would otherwise blow the index budget (§10.1).
        return projection;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        // Callers reject non-UTF-8 text before commit; oversized content
        // skips indexing entirely, so this is defensive-last-line only.
        projection.kind = FileKind::Binary;
        return projection;
    };

    let (yaml, body) = split_frontmatter(text);
    let fm = yaml
        .map(brains_format::frontmatter::parse)
        .unwrap_or_default();
    projection.title = extract_title(fm.title.as_deref(), body).unwrap_or_default();
    // Projections contain relationships, not every occurrence in the document.
    // Both indexes key tags by (path, tag) and links by (source, target, kind).
    let mut tags = fm.tags;
    tags.sort_unstable();
    tags.dedup();
    projection.tags_json = serde_json::to_string(&tags).expect("string list serializes");
    let mut links: Vec<_> = extract_links(&path, body)
        .into_iter()
        .map(|link| (link.target, link.kind.as_str()))
        .collect();
    links.sort_unstable();
    links.dedup();
    let links: Vec<_> = links
        .into_iter()
        .map(|(target, kind)| {
            serde_json::json!({
                "target": target,
                "kind": kind,
            })
        })
        .collect();
    projection.links_json = serde_json::to_string(&links).expect("link records serialize");
    projection.body = text.to_owned();
    let _ = vault; // vault is part of the blob key, not the index row
    projection
}
