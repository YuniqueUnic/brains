use crate::{LinkKind, ResolvedLink, extract_links, extract_title};
use brainstore_domain::VaultPath;

#[test]
fn resolves_relative_links() {
    let source = VaultPath::new("projects/rust-ci/index.md").unwrap();
    let links = extract_links(
        &source,
        "[概念](../../concepts/compilation-cache.md)\n[外部](https://example.com)\n[脚本](scripts/x.sh)",
    );
    assert_eq!(links.len(), 3);
    assert_eq!(
        links[0],
        ResolvedLink {
            target: "concepts/compilation-cache.md".into(),
            kind: LinkKind::Internal
        }
    );
    assert_eq!(links[1].kind, LinkKind::External);
    assert_eq!(links[2].kind, LinkKind::Internal);
}

#[test]
fn code_blocks_yield_no_links() {
    let source = VaultPath::new("a.md").unwrap();
    let links = extract_links(&source, "```\n[x](y.md)\n```\n`[i](j.md)`");
    assert!(links.is_empty());
}

#[test]
fn escaping_link_is_recorded_as_outside() {
    let source = VaultPath::new("a/b.md").unwrap();
    let links = extract_links(&source, "[逃逸](../../escape.md)");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::OutsideVault);
}

#[test]
fn title_from_heading_or_frontmatter() {
    assert_eq!(
        extract_title(None, "intro\n\n# 标题\n"),
        Some("标题".into())
    );
    assert_eq!(extract_title(Some("FM"), "# Ignored\n"), Some("FM".into()));
    assert_eq!(extract_title(None, "```\n# not title\n```\n"), None);
}
