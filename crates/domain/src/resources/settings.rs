//! Editable policy snapshots; usage and provider meter identity remain separate.

use super::*;
use serde::{Deserialize, Serialize};

pub const CLOUDFLARE_FREE_PROFILE: &str =
    include_str!("../../resources/cloudflare-free-2026-10-27.json");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceSettings {
    pub revision: String,
    pub enforced: bool,
    pub profile: ResourceProfile,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuleUpdate {
    pub id: String,
    pub limit: ProviderLimit,
    pub policy: UsagePolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BillingCycleUpdate {
    pub id: String,
    pub start: Option<i64>,
    pub end: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceSettingsUpdate {
    pub expected_revision: String,
    pub enforced: bool,
    pub policy: PolicyDefaults,
    pub rules: Vec<RuleUpdate>,
    pub billing_cycles: Vec<BillingCycleUpdate>,
}
