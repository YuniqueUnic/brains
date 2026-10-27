//! Cloud SDK calls stay here; source writes and authorization stay in application.

mod bindings;
mod content;
mod semantic;

pub use content::{CloudConverter, PublicUrlFetcher};
pub use semantic::{CloudSemantic, SemanticConfig};

use brains_domain::{DomainError, DomainResult};
use wasm_bindgen::JsValue;

fn service_error(operation: &str, error: JsValue) -> DomainError {
    let message = error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&error, &JsValue::from_str("message"))
                .ok()
                .and_then(|v| v.as_string())
        })
        .unwrap_or_else(|| "provider operation failed".into());
    DomainError::StorageUnavailable(format!(
        "{operation}: {}",
        message.chars().take(300).collect::<String>()
    ))
}

fn from_js<T: serde::de::DeserializeOwned>(value: JsValue) -> DomainResult<T> {
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| DomainError::StorageUnavailable(format!("invalid provider response: {e}")))
}
