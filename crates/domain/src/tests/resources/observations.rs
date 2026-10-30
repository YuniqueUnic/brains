use super::*;

fn sample(now: time::OffsetDateTime) -> ProviderObservation {
    ProviderObservation {
        resource: Resource {
            service: "d1".into(),
            kind: "database".into(),
        },
        measure: Measure {
            metric: "storage".into(),
            unit: "byte".into(),
        },
        amount: 3_000_000.0,
        evidence: UsageEvidence::Exact,
        observed_at: now.unix_timestamp() - 60,
        expires_at: now.unix_timestamp() + 60,
        source: "fixture account inventory".into(),
    }
}

#[test]
fn account_gauges_follow_saved_aliases_and_do_not_accumulate() {
    let now = time::OffsetDateTime::now_utc();
    let mut profile = profile();
    let observation = sample(now);
    let first = observation
        .usage(&profile, "account", now)
        .unwrap()
        .unwrap();
    let rule = profile
        .rules
        .iter_mut()
        .find(|rule| rule.id == "d1-account-storage")
        .unwrap();
    rule.id = "saved-storage-alias".into();
    rule.measure.unit = "MB".into();
    let second = observation
        .usage(&profile, "account", now)
        .unwrap()
        .unwrap();
    assert_eq!(first.key, second.key);
    assert_eq!(second.rule, "saved-storage-alias");
    assert_eq!(first.consumed, second.consumed);
    assert_eq!(second.consumed, 3_000_000.0);
    assert_eq!(second.observed_at, Some(observation.observed_at));
    assert_eq!(second.expires_at, Some(observation.expires_at));
}

#[test]
fn incompatible_optional_observations_do_not_block_core_configuration() {
    let now = time::OffsetDateTime::now_utc();
    let observation = sample(now);
    let mut profile = profile();
    profile
        .rules
        .iter_mut()
        .find(|rule| rule.id == "d1-account-storage")
        .unwrap()
        .window = Window::Calendar {
        size: 1,
        unit: "day".into(),
        timezone: "UTC".into(),
    };
    assert!(
        observation
            .usage(&profile, "account", now)
            .unwrap()
            .is_none()
    );
    profile.rules.retain(|rule| rule.id != "d1-account-storage");
    assert!(
        observation
            .usage(&profile, "account", now)
            .unwrap()
            .is_none()
    );
}

#[test]
fn expired_samples_retain_provenance_and_invalid_samples_are_rejected() {
    let now = time::OffsetDateTime::now_utc();
    let profile = profile();
    let mut observation = sample(now);
    observation.expires_at = now.unix_timestamp() - 1;
    let usage = observation
        .usage(&profile, "account", now)
        .unwrap()
        .unwrap();
    assert!(usage.expires_at.unwrap() < now.unix_timestamp());
    assert_eq!(usage.source, Some(observation.source.clone()));
    observation.amount = -1.0;
    assert!(observation.usage(&profile, "account", now).is_err());
    observation.amount = 0.0;
    observation.observed_at = now.unix_timestamp() + 1;
    assert!(observation.usage(&profile, "account", now).is_err());
}
