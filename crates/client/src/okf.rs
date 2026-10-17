//! Open Knowledge Format v0.2 interop (issue #20): export a conformant
//! bundle, import one (reserved `index.md`/`log.md` are ordinary files, never
//! concepts), regenerate scope indexes on demand, and a conformance mode for
//! `check`. Spec: OKF §6.1 root-relative links, §8 index files, §4.1/§5/§7
//! frontmatter families.

use std::collections::BTreeMap;
use std::path::Path;

use brains_format::{NoteStatus, split_frontmatter};
use serde::Serialize;

use crate::endpoint::FileEndpoint;
use crate::{ClientError, ClientResult};

/// Reserved OKF file names: generated as directory indexes/logs, imported as
/// ordinary files, never treated as concepts.
pub const INDEX_FILE: &str = "index.md";
pub const LOG_FILE: &str = "log.md";
pub const OKF_VERSION: &str = "0.2";

const DEFAULT_TYPE: &str = "Note";

fn io(error: std::io::Error) -> ClientError {
    ClientError::transport(format!("okf: {error}"))
}

fn invalid(message: &str) -> ClientError {
    ClientError {
        status: 422,
        code: Some("OKF_INVALID".into()),
        message: message.to_owned(),
        current_revision: None,
    }
}

/// Frontmatter fields the exporter reads from a note. The body is never
/// rewritten, so unknown keys round-trip byte-for-byte.
struct NoteFrontmatter {
    title: Option<String>,
    description: Option<String>,
}

fn parse_frontmatter_lines(yaml: &str) -> NoteFrontmatter {
    let fm = brains_format::frontmatter::parse(yaml);
    NoteFrontmatter {
        title: fm.title.clone(),
        description: fm.description.clone(),
    }
}

/// One concept entry of a directory index (OKF §8).
#[derive(Clone, Serialize)]
struct IndexEntry {
    path: String,
    title: String,
    description: String,
}

/// Generate one directory's `index.md` body: a bulleted list of child notes
/// (excluding reserved files) with titles and descriptions, root-relative
/// links (OKF §6.1, §8).
fn render_index(dir: &str, entries: &[IndexEntry]) -> String {
    let heading = if dir.is_empty() {
        "# Index".to_owned()
    } else {
        format!("# Index — {dir}")
    };
    let mut frontmatter = "---\ntype: Index\n".to_owned();
    if !dir.is_empty() {
        frontmatter.push_str(&format!("title: Index — {dir}\n"));
    }
    frontmatter.push_str("---\n\n");
    let mut text = format!("{frontmatter}{heading}\n\n");
    let root = dir.trim_end_matches('/');
    for entry in entries {
        let root_relative = format!("/{root}/{}", entry.path);
        if entry.description.is_empty() {
            text.push_str(&format!("- [{}]({})\n", entry.title, root_relative));
        } else {
            text.push_str(&format!(
                "- [{}]({}) — {}\n",
                entry.title, root_relative, entry.description
            ));
        }
    }
    text.push('\n');
    text
}

/// `bsx export --format okf`: write the current tree as an OKF bundle.
/// Concept files export byte-identical (unknown keys survive round trips);
/// `index.md` per directory is generated from titles/descriptions; `log.md`
/// is generated only when `with_log` is set, date-grouped and newest first.
/// Notes missing a `type` are left untouched and reported by `check --okf`
/// instead of being rewritten here — history stays immutable.
pub fn export_okf(
    endpoint: &dyn FileEndpoint,
    output: &Path,
    with_log: bool,
) -> ClientResult<usize> {
    if output.symlink_metadata().is_ok() {
        return Err(invalid("output already exists"));
    }
    let files = endpoint.current_files()?;
    let mut written = 0usize;
    // Collect per-directory concept entries for the indexes.
    let mut by_dir: BTreeMap<String, Vec<IndexEntry>> = BTreeMap::new();
    let mut logs: Vec<(String, String)> = Vec::new(); // (updated_at, line)
    for entry in &files {
        if entry.state != "live" {
            continue;
        }
        let bytes = endpoint.revision_bytes(&entry.path, &entry.revision)?;
        let (dir, name) = match entry.path.rsplit_once('/') {
            Some((dir, name)) => (dir.to_owned(), name.to_owned()),
            None => (String::new(), entry.path.clone()),
        };
        let is_markdown = name.ends_with(".md") || name.ends_with(".markdown");
        let (fm_yaml, _) = if is_markdown {
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| invalid(&format!("{} is not UTF-8", entry.path)))?;
            split_frontmatter(text)
        } else {
            (None, "")
        };
        let fm = fm_yaml.map(parse_frontmatter_lines);
        if is_markdown && name != INDEX_FILE && name != LOG_FILE {
            let title = fm
                .as_ref()
                .and_then(|fm| fm.title.clone())
                .unwrap_or_else(|| name.clone());
            let description = fm
                .as_ref()
                .and_then(|fm| fm.description.clone())
                .unwrap_or_default();
            by_dir.entry(dir.clone()).or_default().push(IndexEntry {
                path: name.clone(),
                title,
                description,
            });
        }
        let target = output.join(&entry.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&target, &bytes).map_err(io)?;
        written += 1;
        logs.push((
            entry.updated_at.clone().unwrap_or_default(),
            entry.path.clone(),
        ));
    }
    // Bundle-root index declares okf_version; children get plain indexes.
    let mut dirs: Vec<String> = by_dir.keys().cloned().collect();
    dirs.push(String::new());
    for dir in dirs {
        let entries = by_dir.get(&dir).cloned().unwrap_or_default();
        let mut body = render_index(&dir, &entries);
        if dir.is_empty() {
            // Root index carries the OKF version declaration.
            let type_line_end = body
                .find("type: Index\n")
                .map(|at| at + "type: Index\n".len())
                .unwrap_or(0);
            body.insert_str(type_line_end, &format!("okf_version: \"{OKF_VERSION}\"\n"));
        }
        let index_path = if dir.is_empty() {
            output.join(INDEX_FILE)
        } else {
            output.join(dir).join(INDEX_FILE)
        };
        if index_path.exists() {
            // A note explicitly named index.md was exported; keep the note
            // (it wins as user content) and skip the generated index there.
            continue;
        }
        std::fs::write(&index_path, body).map_err(io)?;
        written += 1;
    }
    if with_log {
        logs.sort_by(|a, b| b.0.cmp(&a.0));
        let mut body = String::from("---\ntype: Log\n---\n\n# Log\n\n");
        let mut current_date = String::new();
        for (updated_at, path) in &logs {
            let date = updated_at.get(..10).unwrap_or("");
            if date != current_date {
                current_date = date.to_owned();
                body.push_str(&format!("\n## {date}\n\n"));
            }
            body.push_str(&format!("- {path}\n"));
        }
        let log_path = output.join(LOG_FILE);
        if log_path.exists() {
            // A user note named log.md was exported; it wins over the
            // generated log (same policy as index.md).
            written -= 1;
        } else {
            std::fs::write(&log_path, body).map_err(io)?;
            written += 1;
        }
    }
    Ok(written)
}

/// `bsx index [--prefix <dir>]`: regenerate `<dir>/index.md` inside the store
/// from the current listing. Hand-written indexes stop drifting by being
/// rewritten from the authoritative list; the note is committed with the
/// usual CAS rules (create-only when missing, `--if-revision` when present).
pub fn rebuild_scope_index(
    endpoint: &dyn FileEndpoint,
    prefix: &str,
    if_revision: Option<&str>,
) -> ClientResult<String> {
    let prefix = brains_domain::path::normalize_prefix(Some(prefix)).unwrap_or("");
    let files = endpoint.current_files()?;
    let mut entries = Vec::new();
    let dir = prefix.to_owned();
    for entry in &files {
        if entry.state != "live" {
            continue;
        }
        let (entry_dir, name) = match entry.path.rsplit_once('/') {
            Some((d, n)) => (d.to_owned(), n.to_owned()),
            None => (String::new(), entry.path.clone()),
        };
        if entry_dir != dir || name == INDEX_FILE || name == LOG_FILE {
            continue;
        }
        let bytes = endpoint.revision_bytes(&entry.path, &entry.revision)?;
        let title = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| {
                let (yaml, body) = split_frontmatter(text);
                let fm = yaml.map(parse_frontmatter_lines);
                brains_format::extract_title(fm.as_ref().and_then(|fm| fm.title.as_deref()), body)
            })
            .unwrap_or_else(|| name.clone());
        let description = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| split_frontmatter(text).0)
            .and_then(|yaml| brains_format::frontmatter::parse(yaml).description)
            .unwrap_or_default();
        entries.push(IndexEntry {
            path: name,
            title,
            description,
        });
    }
    let body = render_index(&dir, &entries);
    let path = if dir.is_empty() {
        INDEX_FILE.to_owned()
    } else {
        format!("{dir}/{INDEX_FILE}")
    };
    let precondition = match endpoint_current_revision(endpoint, &path)? {
        None => crate::Precondition::Create,
        Some(rev) => match if_revision {
            Some(expected) if expected == rev => crate::Precondition::IfRevision(rev),
            _ => {
                return Err(ClientError {
                    status: 412,
                    code: Some("REVISION_CONFLICT".into()),
                    message: format!("{path} exists at revision {rev}; pass --if-revision"),
                    current_revision: Some(rev),
                });
            }
        },
    };
    endpoint.put(&path, body.as_bytes(), "text/markdown", false, precondition)?;
    Ok(path)
}

fn endpoint_current_revision(
    endpoint: &dyn FileEndpoint,
    path: &str,
) -> ClientResult<Option<String>> {
    Ok(endpoint
        .current_files()?
        .into_iter()
        .find(|entry| entry.path == path && entry.state == "live")
        .map(|entry| entry.revision))
}

/// OKF conformance findings for `bsx check --okf` (issue #20): every
/// non-reserved `.md` has parseable frontmatter with a non-empty `type`;
/// reserved files follow the spec. Missing optional fields, unknown types and
/// broken links stay warnings per the spec.
pub fn okf_conformance(
    endpoint: &dyn FileEndpoint,
    prefix: Option<&str>,
) -> ClientResult<Vec<OkfFinding>> {
    let mut findings = Vec::new();
    let files = endpoint.current_files()?;
    for entry in &files {
        if entry.state != "live" {
            continue;
        }
        if let Some(prefix) = prefix
            && !brains_domain::path::prefix_has(prefix, &entry.path)
        {
            continue;
        }
        let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
        let reserved = name == INDEX_FILE || name == LOG_FILE;
        if !(name.ends_with(".md") || name.ends_with(".markdown")) {
            continue;
        }
        let bytes = endpoint.revision_bytes(&entry.path, &entry.revision)?;
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let (yaml, _) = split_frontmatter(text);
        let Some(yaml) = yaml else {
            findings.push(OkfFinding {
                rule: "okf-frontmatter-missing".into(),
                path: entry.path.clone(),
                hint: "non-reserved markdown needs parseable frontmatter with a type".into(),
            });
            continue;
        };
        let fm = brains_format::frontmatter::parse(yaml);
        if reserved {
            // Reserved files follow the spec: index/log types.
            if fm
                .type_
                .as_deref()
                .is_none_or(|t| t != "Index" && t != "Log")
            {
                findings.push(OkfFinding {
                    rule: "okf-reserved-type".into(),
                    path: entry.path.clone(),
                    hint: format!("reserved file `{name}` must declare type: Index or Log"),
                });
            }
            continue;
        }
        if fm.type_.as_deref().is_none_or(str::is_empty) {
            findings.push(OkfFinding {
                rule: "okf-type-missing".into(),
                path: entry.path.clone(),
                hint: format!("frontmatter needs a non-empty type (default `{DEFAULT_TYPE}`)"),
            });
        }
        if fm.status == NoteStatus::Stable && fm.type_.is_some() {
            // fine: absent status is stable per spec
        }
        for warning in &fm.warnings {
            findings.push(OkfFinding {
                rule: "okf-frontmatter-warning".into(),
                path: entry.path.clone(),
                hint: warning.clone(),
            });
        }
    }
    Ok(findings)
}

#[derive(Clone, Debug, Serialize)]
pub struct OkfFinding {
    pub rule: String,
    pub path: String,
    pub hint: String,
}
