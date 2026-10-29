use std::sync::Arc;

use brains_adapter_cloudflare::{CloudConverter, CloudSemantic, PublicUrlFetcher, SemanticConfig};
use brains_adapter_d1::resources::D1UsageStore;
use brains_adapter_d1::settings::D1ResourceSettingsStore;
use brains_adapter_r2::R2ContentStore;
use brains_application::resources::ResourceBudget;
use brains_domain::{DomainError, DomainResult, resources::ResourceProfile};
use brains_ports::{content::ContentConverter, semantic::SemanticRetriever};
use serde::Deserialize;
use worker::Env;

use crate::WorkerVault;

#[derive(Default, Deserialize)]
#[serde(default)]
struct ServiceConfig {
    document_conversion: bool,
    url_import: bool,
    semantic: Option<SemanticConfig>,
}

/// Only the composition root reads deployment configuration.
pub async fn configure(
    env: &Env,
    vault: &mut WorkerVault,
    store: R2ContentStore,
) -> DomainResult<()> {
    let config: ServiceConfig = match env.var("BRAINS_CLOUD_CONFIG") {
        Ok(raw) => serde_json::from_str(&raw.to_string())
            .map_err(|e| DomainError::InvalidArgument(format!("cloud configuration: {e}")))?,
        Err(_) => ServiceConfig::default(),
    };
    let mut deployed = None;
    if let Ok(count) = env.var("BRAINS_RESOURCE_PROFILE_COUNT") {
        let count = count
            .to_string()
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=32).contains(n))
            .ok_or_else(|| {
                DomainError::InvalidArgument("resource profile requires 1..=32 parts".into())
            })?;
        let mut raw = String::new();
        for index in 0..count {
            raw.push_str(
                &env.var(&format!("BRAINS_RESOURCE_PROFILE_{index}"))
                    .map_err(|_| {
                        DomainError::InvalidArgument("resource profile part is missing".into())
                    })?
                    .to_string(),
            );
        }
        deployed = Some(ResourceProfile::parse(&raw)?);
    }
    let database = || {
        env.d1("BRAINS_INDEX")
            .map_err(|e| DomainError::IndexUnavailable(e.to_string()))
    };
    let accounting = Arc::new(brains_adapter_d1::D1Accounting::default());
    let control = brains_application::resource_settings::ResourceControl::load(
        deployed,
        Arc::new(D1ResourceSettingsStore::new(database()?).with_accounting(accounting.clone())),
        Arc::new(D1UsageStore::new(database()?).with_accounting(accounting)),
    )
    .await?;
    let account = env
        .var("BRAINS_RESOURCE_ACCOUNT")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "deployment".into());
    let budget = Arc::new(
        ResourceBudget::new(
            control.settings.profile.clone(),
            account,
            control.usage.clone(),
        )
        .with_enforcement(control.settings.enforced),
    );
    vault.resources = Some(budget.clone());
    vault.resource_control = Some(Arc::new(control));
    let store = store.with_budget(budget.clone());
    vault.heads = store.clone();
    vault.revisions = store.clone();
    vault.blobs = store.clone();
    vault.index = brains_adapter_d1::D1Index::new(database()?).with_budget(budget.clone());
    if config.document_conversion {
        env.ai("AI").map_err(|_| {
            DomainError::ResourcePolicy("AI binding is required for cloud conversion".into())
        })?;
        vault.converter = Some(
            Arc::new(CloudConverter::new(env.clone(), vault.resources.clone()))
                as Arc<dyn ContentConverter>,
        );
    }
    if config.url_import {
        if vault.converter.is_none() {
            return Err(DomainError::InvalidArgument(
                "URL import requires document conversion".into(),
            ));
        }
        vault.fetcher = Some(Arc::new(PublicUrlFetcher));
    }
    if let Some(config) = config.semantic
        && !env
            .var("SEMANTIC_SEARCH")
            .is_ok_and(|v| v.to_string().eq_ignore_ascii_case("off"))
    {
        let semantic = CloudSemantic::new(
            env.clone(),
            database()?,
            store,
            config,
            &vault.vault,
            budget,
        )?;
        vault.semantic = Some(Arc::new(semantic) as Arc<dyn SemanticRetriever>);
    }
    Ok(())
}
