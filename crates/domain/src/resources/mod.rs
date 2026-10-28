//! Provider facts, application policy and usage are separate values.

mod profile;
mod quantity;
mod settings;
mod window;

pub use profile::*;
pub use quantity::*;
pub use settings::*;
pub use window::*;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub service: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measure {
    pub metric: String,
    pub unit: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Account,
    Resource,
    Invocation,
    Record,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceScope {
    pub kind: ScopeKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderLimit {
    Allowance { value: f64 },
    Ceiling { value: f64 },
    Reference { value: f64 },
    Unmetered,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionAction {
    #[default]
    Block,
    Observe,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDefaults {
    pub fraction: Option<f64>,
    pub when_exhausted: Option<AdmissionAction>,
    pub when_charge_unknown: Option<AdmissionAction>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsagePolicy {
    pub fraction: Option<f64>,
    pub maximum: Option<f64>,
    pub when_exhausted: Option<AdmissionAction>,
    pub when_charge_unknown: Option<AdmissionAction>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageEvidence {
    Exact,
    #[default]
    Bounded,
    Estimated,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BudgetCharge {
    pub key: String,
    pub meter: String,
    pub rule: String,
    pub measure: Measure,
    pub amount: f64,
    pub limit: f64,
    pub window: UsageWindow,
    pub evidence: UsageEvidence,
    pub action: AdmissionAction,
    #[serde(default)]
    pub mode: ChargeMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeMode {
    #[default]
    Increment,
    Gauge,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reservation {
    pub attempt_id: String,
    pub charges: Vec<BudgetCharge>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BudgetUsage {
    pub key: String,
    pub meter: String,
    pub rule: String,
    pub unit: String,
    #[serde(default)]
    pub evidence: UsageEvidence,
    pub consumed: f64,
    pub reserved: f64,
    pub limit: f64,
    pub reset_at: Option<i64>,
}

impl BudgetUsage {
    pub fn remaining(&self) -> f64 {
        (self.limit - self.consumed - self.reserved).max(0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Settlement {
    /// The reservation's conservative bound is consumed once.
    Consume,
    /// The request was definitely not sent or charged.
    Release,
    /// Delivery or actual charge is unresolved; retain the reservation.
    Uncertain,
}
