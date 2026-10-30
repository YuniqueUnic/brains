//! The wire boundary exposes primitive rules and scoped usage, not secrets.

use crate::{
    context::current,
    contract::brains as contract,
    state::{Blob, Head, Idx, Rev, SharedState},
};
use brains_domain::{AccessLevel, DomainError};

pub(super) fn wire<T: serde::de::DeserializeOwned>(
    value: impl serde::Serialize,
) -> Result<T, DomainError> {
    serde_json::from_value(
        serde_json::to_value(value).map_err(|e| DomainError::Internal(e.to_string()))?,
    )
    .map_err(|e| DomainError::Internal(format!("API shape mismatch: {e}")))
}

pub(super) async fn report<H: Head, R: Rev, B: Blob, I: Idx>(
    state: &SharedState<H, R, B, I>,
) -> Result<contract::ResourceReport, DomainError> {
    current().auth.principal().require(AccessLevel::Admin, "")?;
    let control = state
        .vault
        .resource_control
        .as_ref()
        .ok_or(DomainError::CapabilityUnsupported("resource_policy"))?;
    build_report(control, &control.settings, control.source).await
}

async fn build_report(
    control: &brains_application::resource_settings::ResourceControl,
    settings: &brains_domain::resources::ResourceSettings,
    source: &str,
) -> Result<contract::ResourceReport, DomainError> {
    let profile = &settings.profile;
    let mut rules = Vec::new();
    for rule in &profile.rules {
        let mut value =
            serde_json::to_value(rule).map_err(|e| DomainError::Internal(e.to_string()))?;
        let limit = rule.effective_limit(&profile.policy)?;
        let (_, scale) = brains_domain::resources::canonical_unit(&rule.measure.unit)?;
        value["effective_limit"] = limit
            .map(|n| serde_json::json!(n / scale))
            .unwrap_or(serde_json::Value::Null);
        let mode = match (
            rule.resource.service.as_str(),
            rule.measure.metric.as_str(),
            &rule.limit,
        ) {
            ("workers", "requests", brains_domain::resources::ProviderLimit::Allowance { .. })
            | ("r2", "class_a_operations" | "class_b_operations", _) => "attempt-admission",
            ("d1", "rows_read" | "rows_written", _) => "observed-and-ledger-admission",
            ("d1", "storage", brains_domain::resources::ProviderLimit::Allowance { .. }) => {
                "provider-snapshot"
            }
            ("d1", "storage", brains_domain::resources::ProviderLimit::Ceiling { .. }) => {
                "observed-watermark"
            }
            ("ai_search", "semantic_queries", _) => "reserved-attempts",
            ("ai_search", "ingestion", _) | ("workers_ai", "compute", _) => "opaque-cost-policy",
            (_, _, brains_domain::resources::ProviderLimit::Ceiling { .. }) => "platform-catalog",
            _ => "informational",
        };
        value["enforcement"] = serde_json::json!(mode);
        if let Some(source) = rule.source.as_ref().and_then(|id| profile.sources.get(id)) {
            value["source"] = serde_json::json!(source);
        }
        rules.push(wire(value)?);
    }
    let billing_cycles = profile
        .context
        .billing_cycles
        .iter()
        .map(|(id, window)| contract::ResourceBillingCycle {
            id: id.clone(),
            start: window.as_ref().map(|w| w.start),
            end: window.as_ref().and_then(|w| w.end),
        })
        .collect();
    let mut usage = control.usage.usage().await?;
    for observation in &control.observations {
        usage.retain(|item| item.key != observation.key);
        usage.push(observation.clone());
    }
    for item in &mut usage {
        if let Some(rule) = profile.rules.iter().find(|rule| rule.id == item.rule)
            && let Some(limit) = rule.effective_limit(&profile.policy)?
        {
            item.limit = limit;
        }
    }
    Ok(contract::ResourceReport {
        settings: contract::ResourceSettingsSummary {
            revision: settings.revision.clone(),
            enforced: settings.enforced,
            source: source.into(),
            policy: wire(&profile.policy)?,
            billing_cycles,
        },
        profile: contract::ResourceCapability {
            profile_id: profile.profile.id.clone(),
            reviewed_at: profile.profile.reviewed_at.clone(),
            coverage: "controlled application attempts; account-wide and opaque usage remain observational".into(),
        },
        rules,
        usage: wire(usage)?,
        limitations: vec![
            "Review current provider pricing, billing windows and account-shared usage.".into(),
            "Unauthenticated traffic and health checks incur platform request/CPU cost outside the application ledger.".into(),
            "Managed ingestion and opaque model costs require a bound or explicit observation policy.".into(),
            "Stored content and provider background work can consume allowance after requests stop.".into(),
        ],
    })
}

pub(super) async fn update<H: Head, R: Rev, B: Blob, I: Idx>(
    state: &SharedState<H, R, B, I>,
    input: contract::ResourceSettingsInput,
) -> Result<contract::ResourceReport, DomainError> {
    current().auth.principal().require(AccessLevel::Admin, "")?;
    let control = state
        .vault
        .resource_control
        .as_ref()
        .ok_or(DomainError::CapabilityUnsupported("resource_policy"))?;
    let input = serde_json::from_value(
        serde_json::to_value(input).map_err(|e| DomainError::Internal(e.to_string()))?,
    )
    .map_err(|e| DomainError::InvalidArgument(format!("resource settings: {e}")))?;
    let settings = control.update(input).await?;
    build_report(control, &settings, "saved").await
}
