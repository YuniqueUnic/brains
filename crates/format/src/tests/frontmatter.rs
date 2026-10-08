use crate::frontmatter::{parse, split_frontmatter};

#[test]
fn splits_frontmatter_and_body() {
    let doc = "---\ntype: Note\ntags: [rust, ci]\n---\n\n# Body\n";
    let (yaml, body) = split_frontmatter(doc);
    assert_eq!(yaml.unwrap(), "type: Note\ntags: [rust, ci]");
    assert_eq!(body.trim(), "# Body");
}

#[test]
fn handles_crlf_documents() {
    let doc = "---\r\ntype: Note\r\n---\r\nbody";
    let (yaml, body) = split_frontmatter(doc);
    assert_eq!(yaml.unwrap(), "type: Note");
    assert_eq!(body.trim(), "body");
}

#[test]
fn no_frontmatter() {
    let (yaml, body) = split_frontmatter("# just a doc\n---\n");
    assert!(yaml.is_none());
    assert_eq!(body, "# just a doc\n---\n");
}

#[test]
fn unterminated_fence_is_not_frontmatter() {
    let (yaml, body) = split_frontmatter("---\ntype: Note\nno closing fence");
    assert!(yaml.is_none());
    assert!(body.starts_with("---"));
}

#[test]
fn parses_okf_shapes() {
    let fm = parse("type: Note\ntitle: \"缓存策略\"\ntags: [rust, ci]\nokf_version: \"0.2\"");
    assert_eq!(fm.type_.as_deref(), Some("Note"));
    assert_eq!(fm.title.as_deref(), Some("缓存策略"));
    assert_eq!(fm.tags, vec!["rust", "ci"]);
    assert_eq!(fm.okf_version.as_deref(), Some("0.2"));
    assert!(fm.warnings.is_empty());
}

#[test]
fn parses_block_list_and_generated_map() {
    let fm = parse(
        "tags:\n  - rust\n  - ci\ngenerated:\n  by: human:owner\n  at: \"2026-10-07T14:00:00Z\"",
    );
    assert_eq!(fm.tags, vec!["rust", "ci"]);
    assert_eq!(fm.generated_by.as_deref(), Some("human:owner"));
    assert_eq!(fm.generated_at.as_deref(), Some("2026-10-07T14:00:00Z"));
    assert!(fm.warnings.is_empty());
}

#[test]
fn comments_and_quotes() {
    let fm = parse("title: '# not a comment' # trailing\n# whole line");
    assert_eq!(fm.title.as_deref(), Some("# not a comment"));
}

#[test]
fn oversized_frontmatter_warns() {
    let big = format!("title: {}", "x".repeat(20_000));
    let fm = parse(&big);
    assert!(fm.title.is_none());
    assert!(!fm.warnings.is_empty());
}
