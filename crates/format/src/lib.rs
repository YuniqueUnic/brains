//! Content format (design §6, §17.1): YAML frontmatter and standard Markdown
//! link extraction. Parsers never rewrite the original bytes; all limits are
//! explicit and reported as warnings, never silently truncated.

pub mod frontmatter;
pub mod markdown;

pub use frontmatter::{Frontmatter, split_frontmatter};
pub use markdown::{LinkKind, ParsedDocument, ResolvedLink, extract_links, extract_title};

use brains_domain::{DomainError, VaultPath};

/// Parse a full text document into the projection used by the index.
pub fn parse_document(path: &VaultPath, text: &str) -> Result<ParsedDocument, DomainError> {
    let (yaml, body) = split_frontmatter(text);
    let frontmatter = yaml.map(frontmatter::parse).unwrap_or_default();
    let links = extract_links(path, body);
    let title = extract_title(frontmatter.title.as_deref(), body);
    Ok(ParsedDocument {
        frontmatter,
        title,
        links,
    })
}

#[cfg(test)]
mod tests;
