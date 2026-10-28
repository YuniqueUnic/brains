//! Revision-checked administration of a policy snapshot, without clearing usage.

use brains_domain::{DomainError, DomainResult, HexId, resources::*};
use brains_ports::resources::{ResourceSettingsStore, UsageStore};
use std::{collections::BTreeSet, sync::Arc};

pub struct ResourceControl {
    pub settings: ResourceSettings,
    pub source: &'static str,
    pub usage: Arc<dyn UsageStore>,
    store: Arc<dyn ResourceSettingsStore>,
    stored_revision: Option<String>,
}

impl ResourceControl {
    pub async fn load(
        deployed: Option<ResourceProfile>,
        store: Arc<dyn ResourceSettingsStore>,
        usage: Arc<dyn UsageStore>,
    ) -> DomainResult<Self> {
        let saved = store.load().await?;
        let stored_revision = saved.as_ref().map(|s| s.revision.clone());
        let source = if saved.is_some() {
            "saved"
        } else if deployed.is_some() {
            "deployment"
        } else {
            "default"
        };
        let settings = match saved {
            Some(s) => s,
            None => {
                let enforced = deployed.is_some();
                let profile = match deployed {
                    Some(p) => p,
                    None => ResourceProfile::parse(CLOUDFLARE_FREE_PROFILE)?,
                };
                let serialized = serde_json::to_vec(&(enforced, &profile))
                    .map_err(|e| DomainError::Internal(e.to_string()))?;
                use sha2::{Digest, Sha256};
                ResourceSettings {
                    revision: hex::encode(Sha256::digest(serialized)),
                    enforced,
                    profile,
                }
            }
        };
        Ok(Self {
            settings,
            source,
            usage,
            store,
            stored_revision,
        })
    }

    pub async fn update(&self, input: ResourceSettingsUpdate) -> DomainResult<ResourceSettings> {
        if input.expected_revision != self.settings.revision {
            return Err(DomainError::RevisionConflict {
                path: "resource-settings".into(),
                current_revision: Some(self.settings.revision.clone()),
            });
        }
        let mut profile = self.settings.profile.clone();
        profile.policy = input.policy;
        let mut seen = BTreeSet::new();
        for update in input.rules {
            if !seen.insert(update.id.clone()) {
                return Err(DomainError::InvalidArgument(
                    "duplicate resource rule".into(),
                ));
            }
            let rule = profile
                .rules
                .iter_mut()
                .find(|r| r.id == update.id)
                .ok_or_else(|| {
                    DomainError::InvalidArgument(format!("unknown resource rule {}", update.id))
                })?;
            if std::mem::discriminant(&rule.limit) != std::mem::discriminant(&update.limit) {
                return Err(DomainError::InvalidArgument(
                    "provider limit kind cannot be changed by a budget edit".into(),
                ));
            }
            rule.limit = update.limit;
            rule.policy = update.policy;
        }
        let mut seen = BTreeSet::new();
        for cycle in input.billing_cycles {
            if !seen.insert(cycle.id.clone())
                || !profile.context.billing_cycles.contains_key(&cycle.id)
            {
                return Err(DomainError::InvalidArgument(
                    "unknown or duplicate billing cycle".into(),
                ));
            }
            let value = match (cycle.start, cycle.end) {
                (None, None) => None,
                (Some(start), Some(end)) if end > start => Some(UsageWindow {
                    start,
                    end: Some(end),
                }),
                _ => {
                    return Err(DomainError::InvalidArgument(
                        "billing period needs a start before its end".into(),
                    ));
                }
            };
            if profile
                .context
                .billing_cycles
                .get(&cycle.id)
                .and_then(Option::as_ref)
                != value.as_ref()
            {
                let rules = profile.rules.iter().filter(|rule| {
                    matches!(&rule.window, Window::BillingCycle { r#ref } if r#ref == &cycle.id)
                }).map(|rule| rule.id.clone()).collect::<Vec<_>>();
                if let Some(end) = self.usage.latest_period_end(&rules).await?
                    && value.as_ref().is_none_or(|window| window.start < end)
                {
                    return Err(DomainError::InvalidArgument("This billing period already has recorded usage. Keep its dates or enter the next non-overlapping period.".into()));
                }
            }
            profile.context.billing_cycles.insert(cycle.id, value);
        }
        let raw =
            serde_json::to_string(&profile).map_err(|e| DomainError::Internal(e.to_string()))?;
        let profile = ResourceProfile::parse(&raw)?;
        if input.enforced {
            for (service, metric) in [
                ("workers", "requests"),
                ("r2", "class_a_operations"),
                ("r2", "class_b_operations"),
                ("d1", "rows_read"),
                ("d1", "rows_written"),
            ] {
                profile
                    .charge(
                        service,
                        metric,
                        0.0,
                        "configuration-validation",
                        UsageEvidence::Exact,
                        time::OffsetDateTime::now_utc(),
                    )
                    .map_err(|e| DomainError::InvalidArgument(e.to_string()))?;
            }
        }
        let next = ResourceSettings {
            revision: HexId::generate().as_str().to_owned(),
            enforced: input.enforced,
            profile,
        };
        self.store
            .save(self.stored_revision.as_deref(), &next)
            .await?;
        Ok(next)
    }
}
