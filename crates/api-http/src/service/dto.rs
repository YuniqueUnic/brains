//! Domain values -> generated wire DTOs, one pure function per shape. The
//! runtime capability report is assembled here as well.

use crate::contract::brains as contract;
use crate::state::{Blob, Head, Idx, Rev, SharedState};

/// The quoted, strong ETag form every reading/mutating response carries.
pub(crate) fn quoted_etag(revision: impl std::fmt::Display) -> String {
    format!("\"{revision}\"")
}

/// A revision's media type, or the store-wide default for contentless
/// revisions (tombstones).
pub(crate) fn media_type_or_default(content: Option<&brains_domain::ContentRef>) -> String {
    content
        .map(|c| c.media_type.clone())
        .unwrap_or_else(|| "application/octet-stream".into())
}

// -- Payload conversions ---------------------------------------------------

/// Entry fields from an authoritative store row (design §11.1).
pub(crate) fn entry(info: &brains_domain::EntryInfo) -> contract::Entry {
    contract::Entry {
        path: info.path.clone(),
        revision: info.revision.clone(),
        generation: info.generation,
        state: info.state.as_str().to_owned(),
        updated_at: Some(info.updated_at.clone()),
        size: info.size,
        sha256: info.sha256.clone(),
        media_type: info.media_type.clone(),
        kind: info.kind.map(|k| k.as_str().to_owned()),
        title: info.title.clone(),
        meta: note_meta(info.meta_json.as_deref()),
    }
}

pub(crate) fn note_meta(meta_json: Option<&str>) -> Option<contract::NoteMeta> {
    brains_api_types::NoteMeta::from_projection(meta_json).map(|meta| contract::NoteMeta {
        r#type: meta.r#type,
        description: meta.description,
        status: meta.status,
        trust: meta.trust,
        stale_after: meta.stale_after,
        generated: meta.generated.map(|generated| contract::Generated {
            by: generated.by,
            at: generated.at,
        }),
        verified: meta
            .verified
            .into_iter()
            .map(|v| contract::Verification { by: v.by, at: v.at })
            .collect(),
        sources: meta
            .sources
            .into_iter()
            .map(|source| contract::SourceRef {
                id: source.id,
                resource: source.resource,
                title: source.title,
                last_modified: source.last_modified,
            })
            .collect(),
    })
}

pub(crate) fn mutation_result(
    outcome: &brains_application::MutationOutcome,
) -> contract::MutationResult {
    contract::MutationResult {
        path: outcome.path.clone(),
        revision: outcome.revision.clone(),
        generation: outcome.generation,
        state: outcome.state.as_str().to_owned(),
        updated_at: Some(outcome.updated_at.clone()),
        size: Some(outcome.size),
        sha256: if outcome.sha256.is_empty() {
            None
        } else {
            Some(outcome.sha256.clone())
        },
        media_type: Some(outcome.media_type.clone()),
        kind: Some(outcome.kind.as_str().to_owned()),
        title: None,
        index_state: outcome.index_state.as_str().to_owned(),
        operation_id: outcome.operation_id.clone(),
        created: Some(outcome.created),
    }
}

pub(crate) fn revision_dto(
    rev: &brains_domain::Revision,
    commit_state: &str,
) -> contract::Revision {
    contract::Revision {
        revision: rev.revision.as_str().to_owned(),
        path: rev.path.clone(),
        generation: rev.generation,
        parent: rev.parent.as_ref().map(|p| p.as_str().to_owned()),
        action: rev.action.as_str().to_owned(),
        actor: Some(rev.actor.clone()),
        operation_id: Some(rev.operation_id.as_str().to_owned()),
        created_at: rev.created_at.clone(),
        restores_revision: rev
            .restores_revision
            .as_ref()
            .map(|r| r.as_str().to_owned()),
        copied_from: rev.copied_from.as_ref().map(|c| contract::CopiedFrom {
            path: c.path.clone(),
            revision: c.revision.as_str().to_owned(),
        }),
        commit_state: Some(commit_state.to_owned()),
        content: rev.content.as_ref().map(|c| contract::ContentRef {
            size: c.size,
            sha256: c.sha256.clone(),
            media_type: c.media_type.clone(),
            executable: c.executable,
        }),
    }
}

pub(crate) fn runtime(runtime: brains_domain::Runtime) -> contract::Runtime {
    match runtime {
        brains_domain::Runtime::LocalRust => contract::Runtime::LocalRust,
        brains_domain::Runtime::CloudflareRust => contract::Runtime::CloudflareRust,
        brains_domain::Runtime::CloudflareTypescript => contract::Runtime::CloudflareTypescript,
    }
}

pub(crate) fn capabilities(
    state: &SharedState<impl Head, impl Rev, impl Blob, impl Idx>,
) -> contract::Capabilities {
    let caps = &state.capabilities;
    contract::Capabilities {
        build_version: caps.build_version.clone(),
        api_version: caps.api_version.clone(),
        store_format: caps.store_format,
        runtime: runtime(caps.runtime),
        limits: contract::CapabilityLimits {
            max_text_bytes: caps.limits.max_text_bytes as u64,
            max_attachment_bytes: caps.limits.max_attachment_bytes as u64,
        },
        features: caps.features.clone().into_iter().collect(),
        semantic: caps
            .semantic
            .as_ref()
            .map(|semantic| contract::SemanticCapability {
                model_id: semantic.model_id.clone(),
                dimensions: semantic.dimensions as u64,
                vector_index_version: semantic.vector_index_version,
                unavailable_reason: semantic.unavailable_reason.clone(),
            }),
        document_conversion: caps.document_conversion.as_ref().map(|conversion| {
            contract::DocumentConversion {
                converter: conversion.converter.clone(),
                version: conversion.version.clone(),
                media_types: conversion.media_types.clone(),
            }
        }),
        vault: caps.vault.clone(),
    }
}

impl From<contract::TransferResult> for contract::TransferConflict {
    fn from(result: contract::TransferResult) -> Self {
        Self {
            copied: result.copied,
            source_trashed: result.source_trashed,
            partial: result.partial,
            operation_id: result.operation_id,
            target_revision: result.target_revision,
            source_revision: result.source_revision,
            error: result.error,
        }
    }
}
