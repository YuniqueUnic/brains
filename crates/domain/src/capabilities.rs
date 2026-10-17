//! Instance capability report (design §11.1, §22.5): every undelivered
//! optional feature is an explicit `false`, and its route answers 501.

use std::collections::BTreeMap;

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
/// set when semantic search is off (no provider, binding missing, disabled).
/// Document conversion of this instance (issue #19).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentConversion {
    pub converter: String,
    pub version: String,
    pub media_types: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticCapability {
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
            ("regex_search".into(), false), // checkout-local rg only (design §10.3)
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
            ("server_side_validate".into(), false), // P1 `brain doctor`
            ("checkout_push_pull".into(), false), // server-side sync endpoint; the CLI composes existing APIs
            ("git_mirror".into(), false),         // manual export only
            ("presigned_attachment_upload".into(), false),
            ("semantic_search".into(), false), // Noop locally; the Worker enables it via bindings
        ]);
        Self {
            build_version: build_version.to_owned(),
            api_version: "1".into(),
            store_format: 1,
            runtime: Runtime::LocalRust,
            limits: CapabilityLimits {
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

    /// The explicit-off semantic report: no model, no projection.
    pub fn semantic_unavailable(reason: &str) -> SemanticCapability {
        SemanticCapability {
            model_id: "none".into(),
            dimensions: 0,
            vector_index_version: 0,
            unavailable_reason: Some(reason.to_owned()),
        }
    }
}
