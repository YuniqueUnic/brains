//! `bsx convert` (issue #19): turn a document attachment into an ordinary,
//! versioned Markdown companion note. Without the build-time `convert`
//! feature the command reports `conversion: unavailable` (exit 9) — it never
//! silently stores less than promised.

use brains_client::{ClientError, ClientResult, Library};

pub(crate) fn convert(client: &dyn Library, path: &str, to: Option<&str>) -> ClientResult<i32> {
    if client.capabilities()?.features.get("remote_conversion") == Some(&true) {
        let report = client.convert_document(path, to)?;
        print_report(&report);
        return Ok(0);
    }
    #[cfg(feature = "convert")]
    {
        use brains_ports::DocumentConverter;
        // Xberg's async pipeline needs a real tokio context; the rest of the
        // Library flow stays outside it, including blocking HTTP operations.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ClientError::transport(e.to_string()))?;
        let converter = brains_adapter_convert::XbergConverter::new();
        let report = brains_client::convert::convert_attachment(
            client,
            &converter,
            path,
            to,
            |bytes, media| runtime.block_on(converter.convert(bytes, media)),
        )?;
        print_report(&report);
        Ok(0)
    }
    #[cfg(not(feature = "convert"))]
    {
        let _ = (client, path, to);
        eprintln!("bsx: conversion: unavailable — this build has no document converter");
        Ok(9)
    }
}

fn print_report(report: &brains_client::convert::ConvertReport) {
    println!(
        "{}: companion {} at revision {} ({})",
        report.converter,
        report.companion_path,
        report.companion_revision,
        if report.unchanged {
            "unchanged"
        } else if report.created {
            "created"
        } else {
            "new revision"
        }
    );
}
