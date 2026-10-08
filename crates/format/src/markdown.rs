//! Standard Markdown link extraction via a real parser (design §6.1, §10.1):
//! relations are expressed by ordinary `[text](target)` links; code spans and
//! code blocks never contribute links. Wikilinks are intentionally not
//! indexed in P0 (design §6.1: ambiguous `[[x]]` must not be auto-resolved).

use pulldown_cmark::{Event, Options, Parser};

use brainstore_domain::VaultPath;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// A resolved vault-internal target.
    Internal,
    /// An absolute URL with a scheme (http:, https:, mailto:, …).
    External,
    /// A relative target escaping the vault — recorded, never silently dropped.
    OutsideVault,
}

impl LinkKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Internal => "link",
            Self::External => "external",
            Self::OutsideVault => "outside",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedLink {
    /// Canonical vault path (internal) or the raw URL (external/outside).
    pub target: String,
    pub kind: LinkKind,
}

#[derive(Clone, Debug, Default)]
pub struct ParsedDocument {
    pub frontmatter: crate::frontmatter::Frontmatter,
    pub title: Option<String>,
    pub links: Vec<ResolvedLink>,
}

/// Extract and resolve standard links relative to `source_path`.
/// Relative targets may contain `../` but must resolve inside the vault.
pub fn extract_links(source_path: &VaultPath, body: &str) -> Vec<ResolvedLink> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(body, options);
    let mut links = Vec::new();
    for event in parser {
        if let Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) = event
            && let Some(link) = resolve_target(source_path, &dest_url)
        {
            links.push(link);
        }
    }
    links
}

fn resolve_target(source_path: &VaultPath, dest: &str) -> Option<ResolvedLink> {
    let dest = dest.trim();
    if dest.is_empty() || dest.starts_with('#') {
        return None;
    }
    if has_scheme(dest) {
        return Some(ResolvedLink {
            target: dest.to_owned(),
            kind: LinkKind::External,
        });
    }
    // Split off a fragment; queries are meaningless on vault paths.
    let (target, _fragment) = match dest.split_once('#') {
        Some((t, f)) => (t, Some(f)),
        None => (dest, None),
    };
    // Resolve relative to the source file's directory: seed the stack with
    // the source directory segments. `../` climbs toward the vault root;
    // climbing past it escapes the vault and is reported.
    let mut segments: Vec<&str> = source_path
        .parent()
        .unwrap_or("")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let mut escaped = false;
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    escaped = true;
                }
            }
            other => segments.push(other),
        }
    }
    if escaped {
        return Some(ResolvedLink {
            target: dest.to_owned(),
            kind: LinkKind::OutsideVault,
        });
    }
    let resolved = segments.join("/");
    if resolved.is_empty() {
        return None;
    }
    match VaultPath::new(&resolved) {
        // NFC-check the resolved target: non-NFC targets stay visible as
        // outside-vault rather than corrupting the index with dup variants.
        Ok(_) => Some(ResolvedLink {
            target: resolved,
            kind: LinkKind::Internal,
        }),
        Err(_) => Some(ResolvedLink {
            target: dest.to_owned(),
            kind: LinkKind::OutsideVault,
        }),
    }
}

fn has_scheme(dest: &str) -> bool {
    let Some((scheme, rest)) = dest.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && (rest.starts_with("//") || scheme.eq_ignore_ascii_case("mailto"))
}

/// Title precedence: frontmatter `title`, else the first ATX heading in the
/// body (scanning a bounded number of lines, ignoring fenced code).
pub fn extract_title(frontmatter_title: Option<&str>, body: &str) -> Option<String> {
    if let Some(title) = frontmatter_title {
        return Some(title.to_owned());
    }
    let mut in_fence = false;
    for line in body.lines().take(200) {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("# ") {
            let title = title.trim();
            if !title.is_empty() {
                return Some(title.to_owned());
            }
        }
    }
    None
}
