use crate::{DomainError, resources::*};

fn profile() -> ResourceProfile {
    ResourceProfile::parse(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/examples/cloudflare-free-2026-10-27.json"
    )))
    .unwrap()
}

#[test]
fn capacity_measurements_keep_their_identity_when_display_units_change() {
    let mut profile = profile();
    let original = profile
        .capacity("d1", "storage", "database", 10_000_000.0)
        .unwrap();
    let rule = profile
        .rules
        .iter_mut()
        .find(|rule| rule.id == "d1-database-storage")
        .unwrap();
    rule.id = "storage-alias".into();
    rule.measure.unit = "GB".into();
    rule.limit = ProviderLimit::Ceiling { value: 0.5 };
    rule.policy.maximum = Some(0.4);
    let changed = profile
        .capacity("d1", "storage", "database", 10_000_000.0)
        .unwrap();
    assert_eq!(original.key, changed.key);
    assert_eq!(changed.amount, 10_000_000.0);
    assert_eq!(changed.limit, 400_000_000.0);
}

#[test]
fn policy_edits_keep_consumption_identity_and_calendar_reset_changes_only_bucket() {
    let mut profile = profile();
    let now = time::OffsetDateTime::from_unix_timestamp(1_793_059_200).unwrap();
    let first = profile
        .charge(
            "workers",
            "requests",
            1.0,
            "account",
            UsageEvidence::Bounded,
            now,
        )
        .unwrap();
    assert_eq!(first.limit, 80_000.0);
    profile.profile.id = "updated-price-snapshot".into();
    let rule = profile
        .rules
        .iter_mut()
        .find(|rule| rule.id == "worker-requests")
        .unwrap();
    rule.id = "request-alias".into();
    rule.policy.maximum = Some(600.0);
    let changed = profile
        .charge(
            "workers",
            "requests",
            1.0,
            "account",
            UsageEvidence::Bounded,
            now,
        )
        .unwrap();
    assert_eq!(first.key, changed.key);
    assert_eq!(changed.limit, 600.0);
    let next = profile
        .charge(
            "workers",
            "requests",
            1.0,
            "account",
            UsageEvidence::Bounded,
            now + time::Duration::days(1),
        )
        .unwrap();
    assert_ne!(changed.key, next.key);
    assert_eq!(changed.meter, next.meter);
}

#[test]
fn unknown_cost_needs_explicit_observation_and_retains_evidence() {
    let mut profile = profile();
    let now = time::OffsetDateTime::now_utc();
    assert!(matches!(
        profile.charge(
            "workers_ai",
            "compute",
            0.0,
            "account",
            UsageEvidence::Unknown,
            now
        ),
        Err(DomainError::ResourcePolicy(_))
    ));
    profile
        .rules
        .iter_mut()
        .find(|rule| rule.id == "workers-ai-neurons")
        .unwrap()
        .policy
        .when_charge_unknown = Some(AdmissionAction::Observe);
    let observed = profile
        .charge(
            "workers_ai",
            "compute",
            0.0,
            "account",
            UsageEvidence::Unknown,
            now,
        )
        .unwrap();
    assert_eq!(observed.evidence, UsageEvidence::Unknown);
}

#[test]
fn unrelated_unknown_catalog_entries_do_not_block_known_meters() {
    let profile = profile();
    let now = time::OffsetDateTime::now_utc();
    assert!(
        profile
            .charge(
                "workers",
                "requests",
                1.0,
                "account",
                UsageEvidence::Bounded,
                now
            )
            .is_ok()
    );
    assert!(matches!(
        profile.charge(
            "r2",
            "class_a_operations",
            1.0,
            "account",
            UsageEvidence::Bounded,
            now
        ),
        Err(DomainError::ResourcePolicy(_))
    ));
}

#[test]
fn fractional_storage_allowance_is_normalized_before_rounding() {
    let mut profile = profile();
    let rule = profile
        .rules
        .iter_mut()
        .find(|rule| rule.id == "d1-account-storage")
        .unwrap();
    rule.limit = ProviderLimit::Allowance { value: 0.5 };
    assert_eq!(
        rule.effective_limit(&profile.policy).unwrap(),
        Some(400_000_000.0)
    );
}

#[test]
fn cropped_vectors_are_normalized_and_invalid_shapes_are_rejected() {
    assert_eq!(
        crate::retrieval::normalize_prefix(&[3.0, 4.0, 9.0], 2).unwrap(),
        vec![0.6, 0.8]
    );
    assert!(crate::retrieval::normalize_prefix(&[0.0, 0.0], 2).is_err());
    assert!(crate::retrieval::normalize_prefix(&[1.0], 2).is_err());
    assert!(crate::retrieval::normalize_prefix(&[f32::NAN], 1).is_err());
}
