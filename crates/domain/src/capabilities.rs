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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub api_version: String, // "1"
    pub store_format: u64,   // 1
    pub runtime: Runtime,
    pub limits: CapabilityLimits,
    pub features: BTreeMap<String, bool>,
    /// Extension (ADR 0002): the single vault this instance manages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault: Option<String>,
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
        vault: &str,
        max_text_bytes: usize,
        max_attachment_bytes: usize,
    ) -> Self {
        let mut capabilities = Self::local(
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
        vault: &str,
        max_text_bytes: usize,
        max_attachment_bytes: usize,
        full_text: FullTextSearch,
    ) -> Self {
        let features: BTreeMap<String, bool> = BTreeMap::from([
            ("literal_search".into(), true),
            ("fts_search".into(), full_text != FullTextSearch::Disabled),
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
        ]);
        Self {
            api_version: "1".into(),
            store_format: 1,
            runtime: Runtime::LocalRust,
            limits: CapabilityLimits {
                max_text_bytes,
                max_attachment_bytes,
            },
            features,
            vault: Some(vault.to_owned()),
        }
    }
}
