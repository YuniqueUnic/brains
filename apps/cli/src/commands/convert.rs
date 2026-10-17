//! `bsx convert` (issue #19): turn a document attachment into an ordinary,
//! versioned Markdown companion note. Without the build-time `convert`
//! feature the command reports `conversion: unavailable` (exit 9) — it never
//! silently stores less than promised.

use brains_client::{ClientError, ClientResult, Library};

pub(crate) fn convert(client: &dyn Library, path: &str, to: Option<&str>) -> ClientResult<i32> {
    #[cfg(feature = "convert")]
    {
        // Xberg's async pipeline needs a real tokio context; the rest of the
        // flow is the synchronous Library API.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ClientError::transport(e.to_string()))?;
        let converter = brains_adapter_convert::XbergConverter::new();
        let report = runtime.block_on(brains_client::convert::convert_attachment(
            client, &converter, path, to,
        ))?;
        println!(
            "{}: companion {} at revision {} ({})",
            report.converter,
            report.companion_path,
            report.companion_revision,
            if report.created {
                "created"
            } else {
                "new revision"
            }
        );
        Ok(0)
    }
    #[cfg(not(feature = "convert"))]
    {
        let _ = (client, path, to);
        eprintln!("bsx: conversion: unavailable — this build has no document converter");
        Ok(9)
    }
}
