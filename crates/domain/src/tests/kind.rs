use crate::{FileKind, detect_kind, is_text_media, media_type_for};

#[test]
fn classifies_common_files() {
    assert_eq!(detect_kind("a/b.md", b"# hi"), FileKind::Note);
    assert_eq!(detect_kind("scripts/x.sh", b"#!/bin/sh"), FileKind::Script);
    assert_eq!(detect_kind("data.csv", b"a,b"), FileKind::Text);
    assert_eq!(detect_kind("img.png", b"\x89PNG"), FileKind::Binary);
    // A "script" that is not UTF-8 must not be indexed as text.
    assert_eq!(detect_kind("s.py", b"\xff\xfe"), FileKind::Binary);
}

#[test]
fn media_types_are_safe_defaults() {
    assert_eq!(media_type_for("a.md"), "text/markdown; charset=utf-8");
    assert_eq!(media_type_for("photo.PNG"), "image/png");
    assert_eq!(media_type_for("blob.bin"), "application/octet-stream");
    assert_eq!(media_type_for("noext"), "application/octet-stream");
}

#[test]
fn text_media_classification_drives_digest_rule() {
    assert!(is_text_media("text/markdown; charset=utf-8"));
    assert!(is_text_media("application/json"));
    assert!(!is_text_media("application/x-www-form-urlencoded"));
    assert!(!is_text_media("image/png"));
}
