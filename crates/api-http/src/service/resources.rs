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
    let budget = state
        .vault
        .resources
        .as_ref()
        .ok_or(DomainError::CapabilityUnsupported("resource_policy"))?;
    let mut rules = Vec::new();
    for rule in &budget.profile.rules {
        let mut value =
            serde_json::to_value(rule).map_err(|e| DomainError::Internal(e.to_string()))?;
        let limit = rule.effective_limit(&budget.profile.policy)?;
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
            ("d1", "storage", brains_domain::resources::ProviderLimit::Ceiling { .. }) => {
                "observed-watermark"
            }
            ("ai_search", "semantic_queries", _) => "reserved-attempts",
            ("ai_search", "ingestion", _) | ("workers_ai", "compute", _) => "opaque-cost-policy",
            (_, _, brains_domain::resources::ProviderLimit::Ceiling { .. }) => "platform-catalog",
            _ => "informational",
        };
        value["enforcement"] = serde_json::json!(mode);
        rules.push(wire(value)?);
    }
    Ok(contract::ResourceReport {
        profile:contract::ResourceCapability {profile_id:budget.profile.profile.id.clone(),reviewed_at:budget.profile.profile.reviewed_at.clone(),coverage:"controlled application attempts; account-wide and opaque usage remain observational".into()},
        rules,
        usage:wire(budget.usage().await?)?,
        limitations:vec!["Review current provider pricing, billing windows and account-shared usage.".into(),"Managed ingestion and opaque model costs require a bound or explicit observation policy.".into(),"Stored content and provider background work can consume allowance after requests stop.".into()],
    })
}
