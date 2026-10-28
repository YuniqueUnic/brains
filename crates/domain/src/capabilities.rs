//! Instance capability report (design §11.1, §22.5): every undelivered
//! optional feature is an explicit `false`, and its route answers 501.

use std::collections::BTreeMap;

mod resources;
pub use resources::{AdmissionState, ResourceCapability};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Runtime {
    LocalRust,
    CloudflareRust,
    CloudflareTypescript,
}

/// Query grammar supplied by the instance's derived index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullTextSearch {
    Disabled,
    Fts5,
    Tantivy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<BTreeMap<String, AdmissionState>>,
    /// Product version and build metadata supplied by the running application.
    pub build_version: String,
    pub api_version: String, // "1"
    pub store_format: u64,   // 1
    pub runtime: Runtime,
    pub limits: CapabilityLimits,
    pub features: BTreeMap<String, bool>,
    /// Semantic search projection state (issue #17): the active model and
    /// vector index version, or an explicit reason when unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic: Option<SemanticCapability>,
    /// Embedded document conversion (issue #19); absent = this build ships
    /// no converter and conversion requests fail clearly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_conversion: Option<DocumentConversion>,
    /// Extension (ADR 0002): the single vault this instance manages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<String>,
}

/// The semantic projection as reported by capabilities: model identity,
/// dimensions and the disposable index's version. `unavailable_reason` is
/// set whenever the runtime cannot serve semantic search (no provider
/// mounted, or disabled).
/// Document conversion of this instance (issue #19).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentConversion {
    pub converter: String,
    pub version: String,
    pub media_types: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension_control: Option<crate::retrieval::DimensionControl>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_modalities: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_inputs: Option<Vec<String>>,
    pub model_id: String,
    pub dimensions: usize,
    pub vector_index_version: i64,
    /// Populated when semantic search is unavailable; capabilities report
    /// the reason instead of failing silently.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CapabilityLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reindex_page_max: Option<usize>,
    pub max_text_bytes: usize,
    pub max_attachment_bytes: usize,
}

impl Capabilities {
    /// The cloudflare-rust profile (design §22.1): R2/D1 adapters deliver the
    /// same P0+P1(partial) feature set on the shared axum layer.
    pub fn cloudflare_rust(
        build_version: &str,
        vault: &str,
        max_text_bytes: usize,
        max_attachment_bytes: usize,
    ) -> Self {
        let mut capabilities = Self::local(
            build_version,
            vault,
            max_text_bytes,
            max_attachment_bytes,
            FullTextSearch::Fts5,
        );
        capabilities.runtime = Runtime::CloudflareRust;
        capabilities
    }

    /// The local-rust profile's P0+P1(partial) feature set.
    pub fn local(
        build_version: &str,
        vault: &str,
        max_text_bytes: usize,
        max_attachment_bytes: usize,
        full_text: FullTextSearch,
    ) -> Self {
        let features: BTreeMap<String, bool> = BTreeMap::from([
            ("literal_search".into(), true),
            ("fts_search".into(), full_text != FullTextSearch::Disabled),
            ("fts_tantivy".into(), full_text == FullTextSearch::Tantivy),
            ("history".into(), true),
            ("restore".into(), true),
            ("trash".into(), true),
            ("patch".into(), true),
            ("copy".into(), true),
            ("move".into(), true),
            ("diff".into(), true),
            ("export".into(), true),
            ("archive".into(), true),
            ("reindex".into(), true),
            ("semantic_search".into(), false), // no runtime mounts an embedding provider yet
        ]);
        Self {
            resources: None,
            admission: None,
            build_version: build_version.to_owned(),
            api_version: "1".into(),
            store_format: 1,
            runtime: Runtime::LocalRust,
            limits: CapabilityLimits {
                reindex_page_max: None,
                max_text_bytes,
                max_attachment_bytes,
            },
            features,
            // Local/native has no embedding provider (issue #17): the port
            // exists, the runtime reports it explicitly unavailable.
            semantic: Some(Self::semantic_unavailable(
                "no embedding provider in the native runtime",
            )),
            document_conversion: None,
            vault: Some(vault.to_owned()),
        }
    }

    /// The document conversion report for builds WITH the embedded
    /// converter (issue #19).
    pub fn document_conversion(
        converter: &str,
        version: &str,
        media_types: &[&str],
    ) -> DocumentConversion {
        DocumentConversion {
            converter: converter.to_owned(),
            version: version.to_owned(),
            media_types: media_types.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    /// Advertise the embedded document converter on this capability set:
    /// sets the block and flips the feature flag in one place, so the three
    /// runtimes (CLI, server, future) cannot drift.
    pub fn with_document_conversion(
        mut self,
        (converter, version): (&str, &str),
        media_types: &[&str],
    ) -> Self {
        self.document_conversion = Some(Self::document_conversion(converter, version, media_types));
        self.features.insert("document_conversion".into(), true);
        self
    }

    /// Drop advertised capabilities by name (the server's BRAINS_DISABLE).
    /// Only honest names are accepted: a capability that cannot actually be
    /// withheld at runtime must not be advertised off (fts is compile-time).
    pub fn without_advertised(mut self, names: &[String]) -> Result<Self, String> {
        for name in names {
            match name.as_str() {
                "document_conversion" => {
                    self.document_conversion = None;
                    self.features.insert(name.clone(), false);
                }
                "semantic" | "semantic_search" => {
                    self.semantic = None;
                    self.features.insert("semantic_search".into(), false);
                }
                other => return Err(other.to_owned()),
            }
        }
        Ok(self)
    }

    /// The explicit-off semantic report: no model, no projection.
    pub fn semantic_unavailable(reason: &str) -> SemanticCapability {
        SemanticCapability {
            dimension_control: None,
            model_modalities: None,
            accepted_inputs: None,
            model_id: "none".into(),
            dimensions: 0,
            vector_index_version: 0,
            unavailable_reason: Some(reason.to_owned()),
        }
    }
}
