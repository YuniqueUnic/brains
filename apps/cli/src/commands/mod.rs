//! CLI command handlers, grouped by user task.

pub(crate) mod check;
pub(crate) mod checkpoint;
pub(crate) mod convert;
pub(crate) mod files;
pub(crate) mod query;
pub(crate) mod sync;
pub(crate) mod system;
pub(crate) mod url_import;
pub(crate) mod working_copy;

use brains_client::{ClientError, ClientResult};

pub(crate) fn print_pretty<T: serde::Serialize>(value: &T) -> ClientResult<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| ClientError::transport(e.to_string()))?
    );
    Ok(())
}
