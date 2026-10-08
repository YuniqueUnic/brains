use crate::{Head, HeadState, HexId};

fn sample() -> Head {
    Head {
        schema_version: 1,
        path: "projects/a.md".into(),
        revision: HexId::from_hex("8aaaaaaaaaaaaaaaaaaaaaaaaaaaaa02").unwrap(),
        generation: 2,
        state: HeadState::Live,
        updated_at: "2026-10-07T14:00:00Z".into(),
    }
}

#[test]
fn json_shape_matches_schema() {
    let json = serde_json::to_value(sample()).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["state"], "live");
    assert_eq!(json["revision"], "8aaaaaaaaaaaaaaaaaaaaaaaaaaaaa02");
    assert_eq!(json["generation"], 2);
    let round: Head = serde_json::from_value(json).unwrap();
    assert_eq!(round.path, "projects/a.md");
}

#[test]
fn rejects_bad_generation() {
    let mut head = sample();
    head.generation = 0;
    assert!(head.validate().is_err());
}

#[test]
fn hex_ids_are_strict() {
    assert!(HexId::from_hex(&"a".repeat(32)).is_ok());
    assert!(HexId::from_hex(&"A".repeat(32)).is_err());
    assert!(HexId::from_hex(&"a".repeat(31)).is_err());
    assert_eq!(HexId::generate().as_str().len(), 32);
}
