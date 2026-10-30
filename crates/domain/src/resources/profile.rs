use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use super::*;
use crate::{DomainError, DomainResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileIdentity {
    pub id: String,
    pub provider: String,
    pub plan: String,
    pub reviewed_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceRule {
    pub id: String,
    pub resource: Resource,
    pub measure: Measure,
    pub scope: ResourceScope,
    pub window: Window,
    pub limit: ProviderLimit,
    #[serde(default)]
    pub policy: UsagePolicy,
    pub source: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceProfile {
    pub profile: ProfileIdentity,
    #[serde(default)]
    pub context: ResourceContext,
    #[serde(default)]
    pub policy: PolicyDefaults,
    pub rules: Vec<ResourceRule>,
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
    #[serde(default)]
    pub changes: Vec<serde_json::Value>,
}

impl ResourceProfile {
    pub fn parse(json: &str) -> DomainResult<Self> {
        let profile: Self = serde_json::from_str(json)
            .map_err(|e| DomainError::InvalidArgument(format!("resource profile: {e}")))?;
        let mut ids = BTreeSet::new();
        for rule in &profile.rules {
            if rule.id.is_empty() || !ids.insert(&rule.id) {
                return Err(DomainError::InvalidArgument(
                    "rule IDs must be nonempty and unique".into(),
                ));
            }
            canonical_unit(&rule.measure.unit)?;
            rule.effective_limit(&profile.policy)?;
            if let Window::Calendar { size, .. } | Window::Rolling { size, .. } = &rule.window
                && *size == 0
            {
                return Err(DomainError::InvalidArgument(format!(
                    "rule {} has an empty window",
                    rule.id
                )));
            }
        }
        Ok(profile)
    }

    pub fn charge(
        &self,
        service: &str,
        metric: &str,
        amount: f64,
        account: &str,
        evidence: UsageEvidence,
        now: OffsetDateTime,
    ) -> DomainResult<BudgetCharge> {
        let rule = self
            .rules
            .iter()
            .find(|r| {
                r.resource.service == service
                    && r.measure.metric == metric
                    && r.scope.kind == ScopeKind::Account
                    && matches!(r.limit, ProviderLimit::Allowance { .. })
            })
            .ok_or_else(|| {
                DomainError::ResourcePolicy(format!("no account allowance for {service}/{metric}"))
            })?;
        if matches!(evidence, UsageEvidence::Unknown | UsageEvidence::Estimated)
            && rule
                .policy
                .when_charge_unknown
                .or(self.policy.when_charge_unknown)
                .unwrap_or_default()
                == AdmissionAction::Block
        {
            return Err(DomainError::ResourcePolicy(format!(
                "unbounded charge for {}",
                rule.id
            )));
        }
        let window = rule.window.resolve(now, &self.context)?;
        let (base_unit, _) = canonical_unit(&rule.measure.unit)?;
        // Aliases and policy values do not identify consumption.
        let identity = serde_json::to_vec(&(
            &self.profile.provider,
            &rule.resource,
            &rule.measure.metric,
            base_unit,
            account,
        ))
        .map_err(|e| DomainError::Internal(e.to_string()))?;
        let meter = hex::encode(Sha256::digest(identity));
        let key = serde_json::to_vec(&(&meter, window.start, window.end))
            .map_err(|e| DomainError::Internal(e.to_string()))?;
        Ok(BudgetCharge {
            key: hex::encode(Sha256::digest(key)),
            meter,
            rule: rule.id.clone(),
            measure: Measure {
                metric: metric.into(),
                unit: base_unit.into(),
            },
            amount: normalize_amount(amount, &rule.measure.unit)?,
            limit: rule.effective_limit(&self.policy)?.ok_or_else(|| {
                DomainError::ResourcePolicy("meter has no effective limit".into())
            })?,
            window,
            evidence,
            action: rule
                .policy
                .when_exhausted
                .or(self.policy.when_exhausted)
                .unwrap_or_default(),
            block_unknown: rule
                .policy
                .when_charge_unknown
                .or(self.policy.when_charge_unknown)
                .unwrap_or_default()
                == AdmissionAction::Block,
            mode: ChargeMode::Increment,
        })
    }

    pub fn capacity(
        &self,
        service: &str,
        metric: &str,
        resource: &str,
        amount: f64,
    ) -> DomainResult<BudgetCharge> {
        let rule = self
            .rules
            .iter()
            .find(|r| {
                r.resource.service == service
                    && r.measure.metric == metric
                    && r.scope.kind == ScopeKind::Resource
                    && matches!(r.limit, ProviderLimit::Ceiling { .. })
            })
            .ok_or_else(|| {
                DomainError::ResourcePolicy(format!("no capacity rule for {service}/{metric}"))
            })?;
        let (unit, _) = canonical_unit(&rule.measure.unit)?;
        let key = serde_json::to_vec(&(
            &self.profile.provider,
            &rule.resource,
            &rule.measure.metric,
            unit,
            resource,
            "capacity",
        ))
        .map_err(|e| DomainError::Internal(e.to_string()))?;
        Ok(BudgetCharge {
            key: hex::encode(Sha256::digest(&key)),
            meter: hex::encode(Sha256::digest(key)),
            rule: rule.id.clone(),
            measure: Measure {
                metric: metric.into(),
                unit: unit.into(),
            },
            amount: normalize_amount(amount, unit)?,
            limit: rule
                .effective_limit(&self.policy)?
                .ok_or_else(|| DomainError::ResourcePolicy("missing capacity limit".into()))?,
            window: UsageWindow {
                start: 0,
                end: None,
            },
            evidence: UsageEvidence::Exact,
            action: rule
                .policy
                .when_exhausted
                .or(self.policy.when_exhausted)
                .unwrap_or_default(),
            block_unknown: rule
                .policy
                .when_charge_unknown
                .or(self.policy.when_charge_unknown)
                .unwrap_or_default()
                == AdmissionAction::Block,
            mode: ChargeMode::Gauge,
        })
    }
}

impl ResourceRule {
    /// Limits use canonical base units; fractional GB never rounds to zero.
    pub fn effective_limit(&self, defaults: &PolicyDefaults) -> DomainResult<Option<f64>> {
        let (value, default_fraction) = match self.limit {
            ProviderLimit::Allowance { value } => (value, defaults.fraction.unwrap_or(0.8)),
            ProviderLimit::Ceiling { value } => (value, 1.0),
            ProviderLimit::Reference { value } => {
                normalize_amount(value, &self.measure.unit)?;
                return Ok(None);
            }
            ProviderLimit::Unknown | ProviderLimit::Unmetered => return Ok(None),
        };
        let ceiling = normalize_amount(value, &self.measure.unit)?;
        let fraction = self.policy.fraction.unwrap_or(default_fraction);
        if !fraction.is_finite() || fraction < 0.0 {
            return Err(DomainError::InvalidArgument(format!(
                "invalid fraction for {}",
                self.id
            )));
        }
        let effective = match self.policy.maximum {
            Some(maximum) => normalize_amount(maximum, &self.measure.unit)?,
            None => ceiling * fraction,
        };
        if !effective.is_finite()
            || effective > 9_007_199_254_740_991.0
            || (matches!(self.limit, ProviderLimit::Ceiling { .. }) && effective > ceiling)
        {
            return Err(DomainError::InvalidArgument(format!(
                "invalid effective limit for {}",
                self.id
            )));
        }
        Ok(Some(effective.floor()))
    }
}
