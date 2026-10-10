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

#[test]
fn standard_yaml_strings_match_displayed_tags() {
    let fm = parse(
        "title: |\n  多行标题\ntags: ['a,b', '  rust  ', rust, 42]\ndescription: |\n  说明\nlinks:\n  项目: https://example.com\n",
    );
    assert_eq!(fm.title.as_deref(), Some("多行标题"));
    assert_eq!(fm.tags, vec!["a,b", "rust"]);
    assert!(fm.warnings.is_empty());
    assert_eq!(parse("tags: rust").tags, vec!["rust"]);
}

#[test]
fn bom_and_closing_fence_at_eof_keep_metadata_indexable() {
    let (yaml, body) = split_frontmatter("\u{feff}---\r\ntags: rust\r\n---");
    assert!(body.is_empty());
    assert_eq!(parse(yaml.unwrap()).tags, vec!["rust"]);
}

#[test]
fn invalid_or_duplicate_metadata_is_not_partially_indexed() {
    for yaml in [
        "tags: [broken",
        "tags: first\ntags: second",
        "- not a mapping",
    ] {
        let fm = parse(yaml);
        assert!(fm.tags.is_empty());
        assert!(!fm.warnings.is_empty());
    }
}

#[test]
fn yaml_core_schema_does_not_merge_hidden_tags_or_coerce_plain_words() {
    let fm = parse(
        "defaults: &base {tags: [hidden]}\n<<: *base\ntags: [yes, no, on, off, true, false, 42]",
    );
    assert_eq!(fm.tags, vec!["yes", "no", "on", "off"]);
    assert!(
        parse("defaults: &base {tags: [hidden]}\n<<: *base")
            .tags
            .is_empty()
    );
}
