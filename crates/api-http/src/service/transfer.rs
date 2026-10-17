//! Shared body of `copy` and `move`: copy create-only, then conditionally
//! trash the source (design §12). Partial outcomes surface as the typed
//! `TransferConflict`, never as silent success.

use brains_domain::{DomainError, HexId, VaultPath};

use crate::context::current;
use crate::contract::brains as contract;
use crate::state::{Blob, Head, Idx, Rev};

use super::error::detail;
use super::files::FilesService;

pub(super) enum TransferKind {
    Copy,
    Move,
}

/// A transfer's outcomes: in-band data (including partial failures, which
/// the IDL raises as `TransferConflict`), or a domain failure.
pub(super) enum TransferOutcome {
    Done(contract::TransferResult),
    Partial(contract::TransferResult),
    Failed(DomainError),
}

impl<H: Head, R: Rev, B: Blob, I: Idx> FilesService<H, R, B, I> {
    /// Copy and move share one body: validate the transfer, run it, and keep
    /// partial outcomes as data — the IDL raises them as `TransferConflict`.
    pub(super) async fn transfer(
        &self,
        body: contract::TransferBody,
        kind: TransferKind,
    ) -> TransferOutcome {
        let ctx = current();
        let rid = ctx.request_id.as_str();
        let invalid = |message: &str| {
            TransferOutcome::Failed(DomainError::InvalidArgument(message.to_owned()))
        };
        // A header/body operation_id mismatch is a programming error (§22.5).
        if let Some(header_op) = ctx
            .headers
            .get("x-operation-id")
            .and_then(|v| v.to_str().ok())
            && header_op != body.operation_id
        {
            return invalid("X-Operation-ID does not match the body operation_id");
        }
        let Ok(source) = VaultPath::new(&body.source) else {
            return invalid("invalid `source` path");
        };
        let Ok(target) = VaultPath::new(&body.target) else {
            return invalid("invalid `target` path");
        };
        let (Ok(expected_source_revision), Ok(operation_id)) = (
            HexId::from_hex(&body.expected_source_revision),
            HexId::from_hex(&body.operation_id),
        ) else {
            return invalid("operation_id and expected_source_revision must be hex ids");
        };
        let input = brains_application::transfer::TransferInput {
            source,
            target,
            expected_source_revision,
            actor: ctx.auth.principal().actor.clone(),
            operation_id,
        };
        let result = match kind {
            TransferKind::Copy => {
                self.state
                    .vault
                    .copy_file(ctx.auth.principal(), input)
                    .await
            }
            TransferKind::Move => {
                self.state
                    .vault
                    .move_file(ctx.auth.principal(), input)
                    .await
            }
        };
        let result = match result {
            Ok(result) => result,
            Err(e) => return TransferOutcome::Failed(e),
        };
        let mapped = contract::TransferResult {
            copied: result.copied,
            source_trashed: result.source_trashed,
            partial: result.partial,
            operation_id: result.operation_id,
            target_revision: result.target_revision,
            source_revision: result.source_revision,
            error: result.error.map(|e| detail(&e, rid)),
        };
        if mapped.partial {
            TransferOutcome::Partial(mapped)
        } else {
            TransferOutcome::Done(mapped)
        }
    }
}
