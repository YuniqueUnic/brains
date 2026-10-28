use std::sync::Arc;

use brains_application::resources::ResourceBudget;
use brains_domain::{
    DomainError, DomainResult,
    resources::{Settlement, UsageEvidence},
};
use brains_ports::{
    ConvertedDocument,
    content::{ContentConverter, FetchedSource, SourceFetcher},
};
use futures::future::BoxFuture;
use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use worker::{Env, send::SendFuture};

use crate::{bindings, from_js, service_error};

const TYPES: &[&str] = &[
    "application/pdf",
    "text/html",
    "text/plain",
    "text/markdown",
    "text/csv",
    "application/xml",
    "text/xml",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "image/png",
    "image/jpeg",
];

pub struct CloudConverter {
    env: Env,
    budget: Option<Arc<ResourceBudget>>,
}

impl CloudConverter {
    pub fn new(env: Env, budget: Option<Arc<ResourceBudget>>) -> Self {
        Self { env, budget }
    }
}

impl ContentConverter for CloudConverter {
    fn identity(&self) -> (&str, &str) {
        ("cloudflare-to-markdown", "1")
    }
    fn media_types(&self) -> Vec<String> {
        TYPES.iter().map(|v| (*v).into()).collect()
    }
    fn cost_evidence(&self, media: &str) -> UsageEvidence {
        if media.starts_with("image/") {
            UsageEvidence::Unknown
        } else {
            UsageEvidence::Exact
        }
    }
    fn convert<'a>(
        &'a self,
        name: &'a str,
        bytes: &'a [u8],
        media: &'a str,
    ) -> BoxFuture<'a, DomainResult<ConvertedDocument>> {
        Box::pin(SendFuture::new(async move {
            if !TYPES.contains(&media) {
                return Err(DomainError::CapabilityUnsupported(
                    "document_conversion_media_type",
                ));
            }
            if media == "text/plain" || media == "text/markdown" {
                let text = std::str::from_utf8(bytes)
                    .map_err(|_| DomainError::InvalidFormat("text is not UTF-8".into()))?;
                return Ok(ConvertedDocument {
                    markdown: text.into(),
                    title: None,
                    converter_id: "cloudflare-to-markdown",
                    converter_version: "1",
                    warnings: vec![],
                });
            }
            let reservation = match (&self.budget, self.cost_evidence(media)) {
                (Some(budget), UsageEvidence::Unknown) => Some(
                    budget
                        .reserve("workers_ai", "compute", 0.0, UsageEvidence::Unknown)
                        .await?,
                ),
                (None, UsageEvidence::Unknown) => {
                    return Err(DomainError::ResourcePolicy(
                        "image conversion requires an explicit model usage policy".into(),
                    ));
                }
                _ => None,
            };
            let result =
                JsFuture::from(bindings::markdown(self.env.as_ref(), name, bytes, media)).await;
            if let (Some(budget), Some(reservation)) = (&self.budget, &reservation) {
                budget.settle(reservation, Settlement::Uncertain).await?;
            }
            #[derive(serde::Deserialize)]
            struct ResultData {
                format: String,
                data: Option<String>,
                error: Option<String>,
            }
            let result: ResultData =
                from_js(result.map_err(|e| service_error("convert document", e))?)?;
            if result.format == "error" {
                return Err(DomainError::InvalidFormat(
                    result.error.unwrap_or_else(|| "conversion failed".into()),
                ));
            }
            let markdown = result
                .data
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    DomainError::InvalidFormat("converter returned no content".into())
                })?;
            Ok(ConvertedDocument {
                markdown,
                title: None,
                converter_id: "cloudflare-to-markdown",
                converter_version: "1",
                warnings: vec![],
            })
        }))
    }
}

pub struct PublicUrlFetcher;

impl SourceFetcher for PublicUrlFetcher {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        max_bytes: usize,
    ) -> BoxFuture<'a, DomainResult<FetchedSource>> {
        Box::pin(SendFuture::new(async move {
            let value = JsFuture::from(bindings::fetch_public(url, max_bytes))
                .await
                .map_err(|e| fetch_error(e, max_bytes))?;
            let field = |name: &str| {
                Reflect::get(&value, &JsValue::from_str(name))
                    .map_err(|e| service_error("URL response", e))
            };
            let url = field("url")?
                .as_string()
                .ok_or_else(|| DomainError::InvalidFormat("missing source URL".into()))?;
            let media_type = field("media_type")?
                .as_string()
                .ok_or_else(|| DomainError::InvalidFormat("missing source media type".into()))?;
            let bytes = Uint8Array::new(&field("bytes")?).to_vec();
            Ok(FetchedSource {
                url,
                media_type,
                bytes,
            })
        }))
    }
}

fn fetch_error(error: JsValue, maximum: usize) -> DomainError {
    let field = |name: &str| Reflect::get(&error, &JsValue::from_str(name)).ok();
    let message = field("message")
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "URL fetch failed".into());
    match field("kind").and_then(|v| v.as_string()).as_deref() {
        Some("invalid_argument") => DomainError::InvalidArgument(message),
        Some("forbidden") => DomainError::Forbidden(message),
        Some("too_large") => DomainError::TooLarge {
            actual: field("size")
                .and_then(|v| v.as_f64())
                .unwrap_or(maximum as f64 + 1.0) as usize,
            limit: maximum,
        },
        _ => service_error("fetch public URL", error),
    }
}
