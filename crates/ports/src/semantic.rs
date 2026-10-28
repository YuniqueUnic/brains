use brains_domain::{DomainResult, SemanticCapability, retrieval::RankingPolicy};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::FileProjection;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticCandidate {
    pub revision: String,
    pub score: f64,
}

pub struct SemanticCandidates {
    pub items: Vec<SemanticCandidate>,
    pub complete: bool,
}

/// Managed retrieval publishes source projections, not precomputed vectors.
pub trait SemanticRetriever: Send + Sync {
    fn capability(&self) -> SemanticCapability;
    fn ranking(&self) -> &RankingPolicy;
    fn publish<'a>(&'a self, projection: &'a FileProjection) -> BoxFuture<'a, DomainResult<()>>;
    fn remove<'a>(
        &'a self,
        path: &'a str,
        revision: Option<&'a str>,
    ) -> BoxFuture<'a, DomainResult<()>>;
    fn query<'a>(&'a self, text: &'a str) -> BoxFuture<'a, DomainResult<SemanticCandidates>>;
    /// An explicit whole-vault maintenance request may resume paused work,
    /// including deletion jobs whose source HEAD has already been purged.
    fn retry_failed(&self, limit: usize) -> BoxFuture<'_, DomainResult<usize>>;
    fn reconcile(&self) -> BoxFuture<'_, DomainResult<usize>>;
}
