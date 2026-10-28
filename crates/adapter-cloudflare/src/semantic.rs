use std::sync::Arc;

use brains_adapter_r2::R2ContentStore;
use brains_application::resources::ResourceBudget;
use brains_domain::{
    DomainError, DomainResult, HeadState, HexId, SemanticCapability, VaultPath,
    resources::{Settlement, UsageEvidence},
    retrieval::{DimensionControl, RankingPolicy},
};
use brains_ports::{
    FileProjection, HeadStore,
    semantic::{SemanticCandidate, SemanticCandidates, SemanticRetriever},
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use worker::{D1Database, Env, send::SendFuture};

use crate::{bindings, from_js, service_error};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SemanticConfig {
    pub instance: String,
    #[serde(default = "default_model")]
    pub embedding_model: String,
    pub dimensions: Option<usize>,
    #[serde(default)]
    pub external_usage: brains_domain::resources::AdmissionAction,
    #[serde(default)]
    pub ranking: RankingPolicy,
}

fn default_model() -> String {
    "@cf/qwen/qwen3-embedding-0.6b".into()
}

impl SemanticConfig {
    pub fn effective_dimensions(&self) -> DomainResult<usize> {
        let size = match self.embedding_model.as_str() {
            "@cf/qwen/qwen3-embedding-0.6b" | "@cf/baai/bge-m3" | "@cf/baai/bge-large-en-v1.5" => {
                1024
            }
            "@cf/google/embeddinggemma-300m" => 768,
            "google-ai-studio/gemini-embedding-2" | "google-ai-studio/gemini-embedding-001" => 1536,
            _ => {
                return Err(DomainError::InvalidArgument(
                    "model is not in this adapter's verified AI Search model catalog".into(),
                ));
            }
        };
        if self.dimensions.is_some_and(|value| value != size) {
            return Err(DomainError::InvalidArgument(format!(
                "managed AI Search model {} requires {size} dimensions; custom dimensions need a self-managed vector provider",
                self.embedding_model
            )));
        }
        if self.embedding_model.starts_with("google-ai-studio/")
            && self.external_usage != brains_domain::resources::AdmissionAction::Observe
        {
            return Err(DomainError::ResourcePolicy("external model billing is opaque; select external_usage=observe only with an independently configured provider budget".into()));
        }
        self.ranking.validate()?;
        if self.instance.is_empty() {
            return Err(DomainError::InvalidArgument(
                "AI Search instance is required".into(),
            ));
        }
        Ok(size)
    }
}

pub struct CloudSemantic {
    env: Env,
    db: brains_adapter_d1::D1Index,
    heads: R2ContentStore,
    config: SemanticConfig,
    space: String,
    budget: Arc<ResourceBudget>,
}

#[derive(Deserialize)]
struct Item {
    revision: String,
    path: String,
    body: String,
    item_id: Option<String>,
    state: String,
}
#[derive(Deserialize)]
struct RemoteItem {
    id: String,
    #[serde(default)]
    status: String,
}

impl CloudSemantic {
    pub fn new(
        env: Env,
        db: D1Database,
        heads: R2ContentStore,
        config: SemanticConfig,
        vault: &str,
        budget: Arc<ResourceBudget>,
    ) -> DomainResult<Self> {
        let dims = config.effective_dimensions()?;
        let space =
            serde_json::to_string(&(vault, &config.instance, &config.embedding_model, dims, 1))
                .map_err(|e| DomainError::Internal(e.to_string()))?;
        Ok(Self {
            env,
            db: brains_adapter_d1::D1Index::new(db).with_budget(budget.clone()),
            heads,
            config,
            space,
            budget,
        })
    }

    async fn run(&self, sql: &str, params: &[JsValue]) -> DomainResult<()> {
        self.db.execute("semantic projection", sql, params).await
    }

    async fn state(
        &self,
        item: &Item,
        state: &str,
        id: Option<&str>,
        error: Option<&str>,
    ) -> DomainResult<()> {
        self.run("UPDATE semantic_items SET state=?3,item_id=COALESCE(?4,item_id),last_error=?5,updated_at=unixepoch() WHERE space=?1 AND revision=?2",&[js(&self.space),js(&item.revision),js(state),id.map(js).unwrap_or(JsValue::NULL),error.map(js).unwrap_or(JsValue::NULL)]).await
    }

    async fn remote(&self, item: &Item) -> DomainResult<Option<RemoteItem>> {
        let result = match &item.item_id {
            Some(id) => {
                JsFuture::from(bindings::item_info(
                    self.env.as_ref(),
                    &self.config.instance,
                    id,
                ))
                .await
            }
            None => {
                JsFuture::from(bindings::find_item(
                    self.env.as_ref(),
                    &self.config.instance,
                    &item_key(&item.revision),
                ))
                .await
            }
        }
        .map_err(|e| service_error("lookup semantic item", e))?;
        if result.is_null() || result.is_undefined() {
            Ok(None)
        } else {
            from_js(result).map(Some)
        }
    }

    async fn process(&self, item: &Item) -> DomainResult<()> {
        let current = self.heads.get(&VaultPath::new(&item.path)?).await?;
        let live = current.as_ref().is_some_and(|head| {
            head.head.state == HeadState::Live && head.head.revision.as_str() == item.revision
        });
        let remote = self.remote(item).await?;
        if !live {
            if let Some(remote) = remote {
                JsFuture::from(bindings::delete_item(
                    self.env.as_ref(),
                    &self.config.instance,
                    &remote.id,
                ))
                .await
                .map_err(|e| service_error("delete semantic item", e))?;
            }
            self.state(item, "deleted", None, None).await?;
            return self
                .run(
                    "UPDATE semantic_items SET body='' WHERE space=?1 AND revision=?2",
                    &[js(&self.space), js(&item.revision)],
                )
                .await;
        }
        if let Some(remote) = remote {
            match remote.status.as_str() {
                "completed" => return self.state(item, "ready", Some(&remote.id), None).await,
                "queued" | "running" => {
                    return self.state(item, "queued", Some(&remote.id), None).await;
                }
                "error" | "skipped" | "outdated" if item.state == "pending" => {
                    // Only an explicit publication/maintenance request resets
                    // a rejected remote item. A queued poll never recreates it.
                    JsFuture::from(bindings::delete_item(
                        self.env.as_ref(), &self.config.instance, &remote.id,
                    )).await.map_err(|e| service_error("reset explicitly retried semantic item", e))?;
                }
                status => return self.state(item, "failed", Some(&remote.id), Some(
                    &format!("AI Search item stopped with status {status}; fix its source or provider configuration before explicit reindex")
                )).await,
            }
        }
        // Source bytes are an estimate, not provider billable tokens.
        // Strict profiles pause this opaque managed operation before dispatch.
        let reservation = self
            .budget
            .reserve(
                "ai_search",
                "ingestion",
                item.body.len() as f64,
                UsageEvidence::Estimated,
            )
            .await?;
        let result = JsFuture::from(bindings::upload_item(
            self.env.as_ref(),
            &self.config.instance,
            &item_key(&item.revision),
            &item.body,
        ))
        .await;
        self.budget
            .settle(
                &reservation,
                if result.is_ok() {
                    Settlement::Consume
                } else {
                    Settlement::Uncertain
                },
            )
            .await?;
        let remote: RemoteItem =
            from_js(result.map_err(|e| service_error("upload semantic item", e))?)?;
        self.state(item, "queued", Some(&remote.id), None).await
    }
}

impl SemanticRetriever for CloudSemantic {
    fn capability(&self) -> SemanticCapability {
        let dims = self
            .config
            .effective_dimensions()
            .expect("validated semantic configuration");
        let modalities = if self.config.embedding_model == "google-ai-studio/gemini-embedding-2" {
            vec!["text", "image", "audio", "video", "pdf"]
        } else {
            vec!["text"]
        };
        SemanticCapability {
            model_id: self.config.embedding_model.clone(),
            dimensions: dims,
            vector_index_version: 1,
            unavailable_reason: None,
            dimension_control: Some(DimensionControl {
                mode: "fixed".into(),
                supported: vec![dims],
                minimum: None,
                maximum: None,
            }),
            model_modalities: Some(modalities.into_iter().map(str::to_owned).collect()),
            accepted_inputs: Some(vec!["text".into()]),
        }
    }
    fn ranking(&self) -> &RankingPolicy {
        &self.config.ranking
    }
    fn publish<'a>(&'a self, projection: &'a FileProjection) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(async move {
            let head = self.heads.get(&VaultPath::new(&projection.path)?).await?;
            if head.is_none_or(|head| head.head.revision.as_str() != projection.revision) {
                return Ok(());
            }
            self.run("UPDATE semantic_items SET state='deleting',updated_at=0 WHERE space=?1 AND path=?2 AND revision<>?3 AND state<>'deleted'",&[js(&self.space),js(&projection.path),js(&projection.revision)]).await?;
            if projection.deleted || projection.body.is_empty() || !projection.kind.searchable() {
                return Ok(());
            }
            self.run("INSERT INTO semantic_items(space,revision,path,generation,body,state,updated_at) VALUES(?1,?2,?3,?4,?5,'pending',0) ON CONFLICT(space,revision) DO UPDATE SET body=excluded.body,state='pending',item_id=NULL,updated_at=0",&[js(&self.space),js(&projection.revision),js(&projection.path),JsValue::from_f64(projection.generation as f64),js(&projection.body)]).await
        }))
    }
    fn remove<'a>(
        &'a self,
        path: &'a str,
        revision: Option<&'a str>,
    ) -> BoxFuture<'a, DomainResult<()>> {
        Box::pin(SendFuture::new(async move {
            self.run("UPDATE semantic_items SET state='deleting',updated_at=0 WHERE space=?1 AND path=?2 AND (?3 IS NULL OR revision=?3) AND state<>'deleted'",&[js(&self.space),js(path),revision.map(js).unwrap_or(JsValue::NULL)]).await
        }))
    }
    fn query<'a>(&'a self, text: &'a str) -> BoxFuture<'a, DomainResult<SemanticCandidates>> {
        Box::pin(SendFuture::new(async move {
            let reservation = self
                .budget
                .reserve("ai_search", "semantic_queries", 1.0, UsageEvidence::Bounded)
                .await?;
            let result = JsFuture::from(bindings::search_query(
                self.env.as_ref(),
                &self.config.instance,
                text,
                self.config.ranking.candidate_limit,
                self.config.ranking.minimum_score.unwrap_or(0.0),
            ))
            .await;
            self.budget
                .settle(
                    &reservation,
                    if result.is_ok() {
                        Settlement::Consume
                    } else {
                        Settlement::Uncertain
                    },
                )
                .await?;
            #[derive(Deserialize)]
            struct Search {
                chunks: Vec<Chunk>,
            }
            #[derive(Deserialize)]
            struct Chunk {
                score: f64,
                item: Key,
            }
            #[derive(Deserialize)]
            struct Key {
                key: String,
            }
            let result: Search = from_js(result.map_err(|e| service_error("semantic query", e))?)?;
            #[derive(Deserialize)]
            struct Coverage {
                incomplete: i64,
            }
            let coverage:Vec<Coverage>=self.db.query("semantic coverage","SELECT EXISTS(SELECT 1 FROM semantic_items WHERE space=?1 AND state IN ('pending','queued','blocked','failed')) AS incomplete",&[js(&self.space)]).await?;
            let complete = result.chunks.len() < self.config.ranking.candidate_limit
                && coverage.first().is_none_or(|row| row.incomplete == 0);
            let mut items = Vec::new();
            for chunk in result.chunks {
                let Some(revision) = chunk
                    .item
                    .key
                    .strip_prefix("p1/")
                    .and_then(|s| s.strip_suffix(".md"))
                else {
                    continue;
                };
                if chunk.score.is_finite() && HexId::from_hex(revision).is_ok() {
                    items.push(SemanticCandidate {
                        revision: revision.into(),
                        score: chunk.score,
                    });
                }
            }
            items.sort_by(|a, b| {
                b.score
                    .total_cmp(&a.score)
                    .then_with(|| a.revision.cmp(&b.revision))
            });
            let mut seen = std::collections::BTreeSet::new();
            items.retain(|item| seen.insert(item.revision.clone()));
            Ok(SemanticCandidates { items, complete })
        }))
    }
    fn retry_failed(&self, limit: usize) -> BoxFuture<'_, DomainResult<usize>> {
        Box::pin(SendFuture::new(async move {
            let rows: Vec<serde_json::Value> = self.db.execute_returning(
                "resume paused semantic work",
                "UPDATE semantic_items SET state=CASE WHEN state='delete_failed' THEN 'deleting' ELSE 'pending' END,item_id=NULL,last_error=NULL,updated_at=0 WHERE (space,revision) IN (SELECT space,revision FROM semantic_items WHERE space=?1 AND state IN ('failed','blocked','delete_failed') ORDER BY updated_at,revision LIMIT ?2) RETURNING revision",
                &[js(&self.space), JsValue::from_f64(limit as f64)],
            ).await?;
            Ok(rows.len())
        }))
    }

    fn reconcile(&self) -> BoxFuture<'_, DomainResult<usize>> {
        Box::pin(SendFuture::new(async move {
            // updated_at also leases one job. Scheduled and request reconciliation
            // share the claim, and an interrupted Worker makes it eligible again.
            let items:Vec<Item>=self.db.execute_returning("claim semantic projection","UPDATE semantic_items SET updated_at=unixepoch()+90 WHERE (space,revision) IN (SELECT space,revision FROM semantic_items WHERE space=?1 AND state IN ('pending','queued','deleting') AND updated_at<unixepoch()-30 ORDER BY updated_at LIMIT 1) RETURNING revision,path,body,item_id,state",&[js(&self.space)]).await?;
            let mut processed = 0;
            for item in items {
                match self.process(&item).await {
                    Ok(()) => processed += 1,
                    Err(error) => {
                        let state = if item.state == "deleting" {
                            "delete_failed"
                        } else if matches!(
                            error,
                            DomainError::ResourcePolicy(_) | DomainError::QuotaExceeded { .. }
                        ) {
                            "blocked"
                        } else {
                            "failed"
                        };
                        self.state(&item, state, None, Some(&error.to_string()))
                            .await?;
                    }
                }
            }
            Ok(processed)
        }))
    }
}

fn js(value: &str) -> JsValue {
    JsValue::from_str(value)
}
fn item_key(revision: &str) -> String {
    format!("p1/{revision}.md")
}
