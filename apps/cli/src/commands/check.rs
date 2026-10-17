//! `bsx check` (issue #14): read-only quality report with stable `--json`
//! output. Exit codes: 0 clean, 10 findings. `--okf` switches to the OKF
//! v0.2 conformance rules of issue #20.

use brains_client::check::{CheckFinding, run_check};
use brains_client::{ClientError, ClientResult, Library};

pub(crate) fn check(
    client: &dyn Library,
    prefix: Option<&str>,
    json: bool,
    okf: bool,
) -> ClientResult<i32> {
    let today = brains_domain::timestamp::today_utc();
    let (findings, checked, skipped) = if okf {
        let rows = brains_client::okf::okf_conformance(client, prefix)?;
        let count = rows.len();
        let findings = rows
            .into_iter()
            .map(|row| CheckFinding {
                rule: row.rule,
                path: row.path,
                hint: row.hint,
            })
            .collect::<Vec<_>>();
        (findings, count, 0)
    } else {
        let report = run_check(client, prefix, &today)?;
        (report.findings, report.checked, report.skipped)
    };
    if json {
        let payload = serde_json::json!({
            "checked": checked,
            "skipped": skipped,
            "findings": findings,
        });
        serde_json::to_string_pretty(&payload)
            .map_err(|e| ClientError::transport(e.to_string()))?
            .lines()
            .for_each(|line| println!("{line}"));
    } else {
        for finding in &findings {
            println!("{}\t{}\t{}", finding.rule, finding.path, finding.hint);
        }
        eprintln!(
            "checked {checked} files ({skipped} skipped): {} findings",
            findings.len()
        );
    }
    Ok(if findings.is_empty() { 0 } else { 10 })
}
