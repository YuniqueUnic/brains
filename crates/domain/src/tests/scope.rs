use crate::{AccessLevel, Scope};

#[test]
fn scope_parse_roundtrip() {
    assert_eq!(Scope::parse("read:/").unwrap().as_string(), "read:/");
    assert_eq!(
        Scope::parse("write:projects/rust-ci").unwrap().as_string(),
        "write:projects/rust-ci"
    );
}

#[test]
fn scope_prefix_boundaries() {
    let s = Scope::parse("write:projects/foo").unwrap();
    assert!(s.allows(AccessLevel::Write, "projects/foo"));
    assert!(s.allows(AccessLevel::Write, "projects/foo/bar.md"));
    assert!(!s.allows(AccessLevel::Write, "projects/foobar.md"));
    assert!(!s.allows(AccessLevel::Admin, "projects/foo"));
}

#[test]
fn read_scopes_do_not_imply_write() {
    let s = Scope::parse("read:/").unwrap();
    assert!(s.allows(AccessLevel::Read, "anything.md"));
    assert!(!s.allows(AccessLevel::Write, "anything.md"));
}

#[test]
fn trailing_slash_scope_keeps_segment_boundary() {
    let scope = Scope::parse("read:public/").unwrap();
    assert_eq!(scope.as_string(), "read:public");
    for path in ["public", "public/open.md", "public/ÿ/笔记.md"] {
        assert!(scope.allows(AccessLevel::Read, path));
    }
    for path in ["", "public-secret.md", "public2/open.md"] {
        assert!(!scope.allows(AccessLevel::Read, path));
    }
}
