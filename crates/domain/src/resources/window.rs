use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{DomainError, DomainResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Window {
    None,
    Calendar {
        size: u32,
        unit: String,
        timezone: String,
    },
    BillingCycle {
        r#ref: String,
    },
    Rolling {
        size: u32,
        unit: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub start: i64,
    pub end: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceContext {
    #[serde(default)]
    pub billing_cycles: BTreeMap<String, Option<UsageWindow>>,
    pub gateway_log_generation: Option<String>,
}

impl Window {
    pub fn resolve(
        &self,
        now: OffsetDateTime,
        context: &ResourceContext,
    ) -> DomainResult<UsageWindow> {
        match self {
            Self::None => Ok(UsageWindow {
                start: 0,
                end: None,
            }),
            Self::BillingCycle { r#ref } => context
                .billing_cycles
                .get(r#ref)
                .and_then(Clone::clone)
                .filter(|w| {
                    w.start <= now.unix_timestamp()
                        && w.end.is_some_and(|end| now.unix_timestamp() < end)
                })
                .ok_or_else(|| {
                    DomainError::ResourcePolicy(format!(
                        "billing cycle {ref} is unconfigured or expired"
                    ))
                }),
            Self::Calendar {
                size,
                unit,
                timezone,
            } => {
                if timezone != "UTC" {
                    return Err(DomainError::ResourcePolicy(format!(
                        "calendar timezone {timezone} is not supported by this runtime"
                    )));
                }
                let seconds = interval_seconds(*size, unit)?;
                let start = now.unix_timestamp().div_euclid(seconds) * seconds;
                Ok(UsageWindow {
                    start,
                    end: Some(start + seconds),
                })
            }
            Self::Rolling { .. } => Err(DomainError::ResourcePolicy(
                "rolling rate limits are enforced by their provider, not by a fixed-window counter"
                    .into(),
            )),
        }
    }
}

fn interval_seconds(size: u32, unit: &str) -> DomainResult<i64> {
    let base: i64 = match unit {
        "second" => 1,
        "minute" => 60,
        "hour" => 3600,
        "day" => 86400,
        _ => {
            return Err(DomainError::InvalidArgument(format!(
                "unsupported calendar unit: {unit}"
            )));
        }
    };
    if size == 0 {
        return Err(DomainError::InvalidArgument(
            "window size must be positive".into(),
        ));
    }
    Ok(base * i64::from(size))
}
