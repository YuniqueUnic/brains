use crate::{CursorSigner, Endpoint};

#[test]
fn roundtrip_and_tamper() {
    let signer = CursorSigner::new([7u8; 32]);
    let cursor = signer.issue(Endpoint::Search, "q=缓存;prefix=", "a/ z.md");
    assert_eq!(
        signer
            .verify(Endpoint::Search, "q=缓存;prefix=", &cursor)
            .unwrap(),
        "a/ z.md"
    );
    assert!(
        signer
            .verify(Endpoint::Search, "q=other;prefix=", &cursor)
            .is_err()
    );
    assert!(
        signer
            .verify(Endpoint::Files, "q=缓存;prefix=", &cursor)
            .is_err()
    );
    let mut tampered = cursor.clone();
    tampered.replace_range(0..1, "A");
    assert!(
        signer
            .verify(Endpoint::Search, "q=缓存;prefix=", &tampered)
            .is_err()
    );
}
