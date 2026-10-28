use crate::{DomainError, DomainResult};

/// A canonical unit makes display-unit changes preserve the same ledger.
pub fn canonical_unit(unit: &str) -> DomainResult<(&str, f64)> {
    Ok(match unit {
        "GB" => ("byte", 1_000_000_000.0),
        "MB" => ("byte", 1_000_000.0),
        "MiB" => ("byte", 1_048_576.0),
        "byte" => ("byte", 1.0),
        "GB-month" => ("byte-month", 1_000_000_000.0),
        "byte-month" => ("byte-month", 1.0),
        "day" => ("microsecond", 86_400_000_000.0),
        "hour" => ("microsecond", 3_600_000_000.0),
        "second" => ("microsecond", 1_000_000.0),
        "ms" => ("microsecond", 1_000.0),
        "microsecond" => ("microsecond", 1.0),
        "request" | "event" | "operation" | "row" | "neuron" | "token" | "query" | "user"
        | "worker" | "trigger" | "file" | "database" | "instance" | "namespace" | "field"
        | "chunk" | "portal" | "server" | "domain" | "session" | "dimension" | "connection" => {
            (unit, 1.0)
        }
        _ => {
            return Err(DomainError::InvalidArgument(format!(
                "unsupported unit: {unit}"
            )));
        }
    })
}

pub fn normalize_amount(value: f64, unit: &str) -> DomainResult<f64> {
    let (_, multiplier) = canonical_unit(unit)?;
    let amount = value * multiplier;
    if !amount.is_finite() || !(0.0..=9_007_199_254_740_991.0).contains(&amount) {
        return Err(DomainError::InvalidArgument(
            "quantity must be finite, non-negative and at most 2^53 - 1 in base units".into(),
        ));
    }
    Ok(amount)
}
