//! Vector shape and retrieval policy do not depend on a model SDK.

use serde::{Deserialize, Serialize};

use crate::{DomainError, DomainResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DimensionControl {
    pub mode: String,
    pub supported: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RankingPolicy {
    pub candidate_limit: usize,
    pub validation_limit: usize,
    pub result_limit: usize,
    pub minimum_score: Option<f64>,
}

impl Default for RankingPolicy {
    fn default() -> Self {
        Self {
            candidate_limit: 50,
            validation_limit: 8,
            result_limit: 5,
            minimum_score: None,
        }
    }
}

impl RankingPolicy {
    pub fn validate(&self) -> DomainResult<()> {
        if !(1..=50).contains(&self.candidate_limit)
            || self.result_limit == 0
            || self.result_limit > self.validation_limit
            || self.validation_limit > self.candidate_limit
            || self
                .minimum_score
                .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
        {
            return Err(DomainError::InvalidArgument("ranking requires 1..=50 candidates, 1..=candidate_limit results and an optional score in 0..=1".into()));
        }
        Ok(())
    }
}

/// Apply only for an MRL-capable model when the caller owns both vector paths.
pub fn normalize_prefix(vector: &[f32], dimensions: usize) -> DomainResult<Vec<f32>> {
    if dimensions == 0 || dimensions > vector.len() {
        return Err(DomainError::InvalidArgument(
            "invalid embedding prefix dimensions".into(),
        ));
    }
    let prefix = &vector[..dimensions];
    let squared: f64 = prefix.iter().map(|v| f64::from(*v).powi(2)).sum();
    if !squared.is_finite() || squared <= 0.0 {
        return Err(DomainError::InvalidFormat(
            "embedding must contain finite nonzero values".into(),
        ));
    }
    let norm = squared.sqrt();
    Ok(prefix
        .iter()
        .map(|v| (f64::from(*v) / norm) as f32)
        .collect())
}
