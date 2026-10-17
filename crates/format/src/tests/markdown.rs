use crate::{LinkKind, ResolvedLink, extract_links, extract_title};
use brains_domain::VaultPath;

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

#[test]
fn root_relative_links_resolve_from_vault_root() {
    let source = VaultPath::new("notes/sales/q3.md").unwrap();
    let links = extract_links(&source, "[订单表](/tables/orders.md)");
    assert_eq!(
        links,
        vec![ResolvedLink {
            target: "tables/orders.md".into(),
            kind: LinkKind::Internal
        }]
    );
}

#[test]
fn relative_link_forms() {
    let source = VaultPath::new("notes/sales/q3.md").unwrap();
    let links = extract_links(&source, "[同目录](./summary.md) [上级](../pricing.md)");
    assert_eq!(
        links,
        vec![
            ResolvedLink {
                target: "notes/sales/summary.md".into(),
                kind: LinkKind::Internal
            },
            ResolvedLink {
                target: "notes/pricing.md".into(),
                kind: LinkKind::Internal
            },
        ]
    );
}

#[test]
fn root_relative_escape_is_outside_vault() {
    let source = VaultPath::new("notes/sales/q3.md").unwrap();
    let links = extract_links(&source, "[逃逸](/../outside.md)");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::OutsideVault);
}

#[test]
fn fragments_do_not_change_resolution() {
    let source = VaultPath::new("notes/sales/q3.md").unwrap();
    let links = extract_links(
        &source,
        "[根相对锚点](/tables/orders.md#row-3) [同目录锚点](summary.md#top)",
    );
    assert_eq!(
        links,
        vec![
            ResolvedLink {
                target: "tables/orders.md".into(),
                kind: LinkKind::Internal
            },
            ResolvedLink {
                target: "notes/sales/summary.md".into(),
                kind: LinkKind::Internal
            },
        ]
    );
}

#[test]
fn non_nfc_root_relative_target_is_outside() {
    // NFD form of "café" (e + combining acute) must not enter the index as a
    // duplicate variant of the NFC path.
    let source = VaultPath::new("notes/sales/q3.md").unwrap();
    let nfd = "notes/cafe\u{301}.md";
    let links = extract_links(&source, &format!("[非规范](/{nfd})"));
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::OutsideVault);
    assert_eq!(links[0].target, format!("/{nfd}"));
}
