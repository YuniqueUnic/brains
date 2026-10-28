use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceCapability {
    pub profile_id: String,
    pub reviewed_at: String,
    pub coverage: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdmissionState {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<i64>,
}
