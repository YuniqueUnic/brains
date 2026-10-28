//! Explicit semantic maintenance is bounded and requires whole-vault ownership.

use crate::{
    context::current,
    contract::brains as contract,
    state::{Blob, Head, Idx, Rev, SharedState},
};
use brains_domain::{AccessLevel, DomainError};
use brains_ports::semantic::SemanticWork;

pub(super) async fn reconcile<H: Head, R: Rev, B: Blob, I: Idx>(
    state: &SharedState<H, R, B, I>,
    input: contract::SemanticReconcileRequest,
) -> Result<contract::SemanticReconcileResult, DomainError> {
    current().auth.principal().require(AccessLevel::Admin, "")?;
    let semantic = state
        .vault
        .semantic
        .as_ref()
        .ok_or(DomainError::CapabilityUnsupported("semantic_search"))?;
    let limit = input.limit.unwrap_or(1);
    if !(0..=2).contains(&limit) {
        return Err(DomainError::InvalidArgument(
            "limit must be between 0 and 2".into(),
        ));
    }
    let processed = semantic
        .reconcile(SemanticWork::All, limit as usize)
        .await?;
    Ok(contract::SemanticReconcileResult {
        processed: processed as i64,
        states: semantic.states().await?.into_iter().collect(),
    })
}
