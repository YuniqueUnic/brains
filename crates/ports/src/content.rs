use brains_domain::{DomainResult, resources::UsageEvidence};
use futures::future::BoxFuture;

use crate::ConvertedDocument;

/// Async converter boundary usable by the Worker and native implementations.
pub trait ContentConverter: Send + Sync {
    fn identity(&self) -> (&str, &str);
    fn media_types(&self) -> Vec<String>;
    fn cost_evidence(&self, media_type: &str) -> UsageEvidence;
    fn convert<'a>(
        &'a self,
        name: &'a str,
        bytes: &'a [u8],
        media_type: &'a str,
    ) -> BoxFuture<'a, DomainResult<ConvertedDocument>>;
}

pub struct FetchedSource {
    pub url: String,
    pub bytes: Vec<u8>,
    pub media_type: String,
}

pub trait SourceFetcher: Send + Sync {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        max_bytes: usize,
    ) -> BoxFuture<'a, DomainResult<FetchedSource>>;
}
