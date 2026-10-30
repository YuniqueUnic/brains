//! Timestamped provider gauges are facts, separate from limits and counters.

use super::*;
use crate::{DomainError, DomainResult};
use time::OffsetDateTime;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderObservation {
    pub resource: Resource,
    pub measure: Measure,
    pub amount: f64,
    pub evidence: UsageEvidence,
    pub observed_at: i64,
    pub expires_at: i64,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderSnapshot {
    pub account: String,
    pub observations: Vec<ProviderObservation>,
}

impl ProviderObservation {
    pub fn usage(
        &self,
        profile: &ResourceProfile,
        account: &str,
        now: OffsetDateTime,
    ) -> DomainResult<Option<BudgetUsage>> {
        let amount = normalize_amount(self.amount, &self.measure.unit)?;
        if self.observed_at > now.unix_timestamp()
            || self.expires_at <= self.observed_at
            || self.source.trim().is_empty()
        {
            return Err(DomainError::InvalidArgument(
                "invalid provider observation".into(),
            ));
        }
        let rule = profile.rules.iter().find(|rule| {
            rule.resource.service == self.resource.service
                && rule.measure.metric == self.measure.metric
                && rule.scope.kind == ScopeKind::Account
                && matches!(rule.limit, ProviderLimit::Allowance { .. })
        });
        let Some(rule) = rule else {
            return Ok(None);
        };
        // Optional facts follow the effective saved profile, including aliases.
        // An incompatible gauge does not make core API construction unavailable.
        if rule.resource != self.resource
            || !matches!(rule.window, Window::None)
            || canonical_unit(&rule.measure.unit)?.0 != canonical_unit(&self.measure.unit)?.0
        {
            return Ok(None);
        }
        let charge = profile.charge(
            &rule.resource.service,
            &rule.measure.metric,
            0.0,
            account,
            UsageEvidence::Bounded,
            now,
        )?;
        Ok(Some(BudgetUsage {
            key: charge.key,
            meter: charge.meter,
            rule: charge.rule,
            unit: charge.measure.unit,
            consumed: amount,
            reserved: 0.0,
            evidence: self.evidence,
            limit: charge.limit,
            reset_at: None,
            observed_at: Some(self.observed_at),
            expires_at: Some(self.expires_at),
            source: Some(self.source.clone()),
        }))
    }
}
