use crate::{VaultPath, path};

#[test]
fn accepts_normal_paths() {
    assert!(VaultPath::new("projects/rust-ci/cache-strategy.md").is_ok());
    assert!(VaultPath::new("index.md").is_ok());
    assert!(VaultPath::new("中文/缓存策略.md").is_ok());
}

#[test]
fn rejects_unsafe_paths() {
    for bad in [
        "",
        "/abs.md",
        "a/../b.md",
        "./a.md",
        "a//b.md",
        "a\\b.md",
        ".brain/x.md",
        "a\0b",
    ] {
        assert!(VaultPath::new(bad).is_err(), "should reject {bad:?}");
    }
    // Windows-reserved names are legal in the vault; export reports them.
    assert!(VaultPath::new("con.md").is_ok());
}

#[test]
fn prefix_is_segment_bound() {
    assert!(
        VaultPath::new("projects/foo/bar.md")
            .unwrap()
            .has_prefix("projects/foo")
    );
    assert!(
        !VaultPath::new("projects/foobar.md")
            .unwrap()
            .has_prefix("projects/foo")
    );
}

#[test]
fn portable_names_report_at_export() {
    assert!(path::check_portable_segment("con.md").is_err());
    assert!(path::check_portable_segment("a<b.md").is_err());
    assert!(path::check_portable_segment("x. ").is_err());
    assert!(path::check_portable_segment("缓存策略.md").is_ok());
}
