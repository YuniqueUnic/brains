//! Parse bounded YAML frontmatter, then derive the fields used by the index.
//! The original document remains authoritative; unknown properties stay in it.

use serde_json::{Map, Value};
use serde_saphyr::{Budget, DuplicateKeyPolicy, MergeKeyPolicy, Options};

const MAX_FRONTMATTER_BYTES: usize = 16 * 1024;
const MAX_TAGS: usize = 64;
const MAX_VERIFIED: usize = 64;
const MAX_SOURCES: usize = 64;

/// Note lifecycle (OKF §5): absence means `Stable`, never an unknown state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoteStatus {
    Draft,
    #[default]
    Stable,
    Deprecated,
}

impl NoteStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Stable => "stable",
            Self::Deprecated => "deprecated",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "draft" => Some(Self::Draft),
            "stable" => Some(Self::Stable),
            "deprecated" => Some(Self::Deprecated),
            _ => None,
        }
    }
}

/// One independent confirmation (`verified: [{by, at}]`, OKF §5.3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Verification {
    pub by: String,
    pub at: Option<String>,
}

/// One provenance entry (`sources: [{id, resource, title, last_modified}]`,
/// OKF §7). Body footnotes `[^id]` attribute individual claims.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceRef {
    pub id: String,
    pub resource: Option<String>,
    pub title: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frontmatter {
    pub type_: Option<String>,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub okf_version: Option<String>,
    pub generated_by: Option<String>,
    pub generated_at: Option<String>,
    pub description: Option<String>,
    pub status: NoteStatus,
    /// Absolute `YYYY-MM-DD`; the note is stale on or after that day.
    pub stale_after: Option<String>,
    pub verified: Vec<Verification>,
    pub sources: Vec<SourceRef>,
    /// Non-fatal parse findings; the blob keeps the original bytes.
    pub warnings: Vec<String>,
}

/// Trust tier derived from `verified` (OKF §5.3): informational signal,
/// never an authorization.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrustTier {
    #[default]
    Unverified,
    MachineConfirmed,
    HumanReviewed,
}

impl TrustTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unverified => "unverified",
            Self::MachineConfirmed => "machine-confirmed",
            Self::HumanReviewed => "human-reviewed",
        }
    }
}

impl Frontmatter {
    pub fn trust_tier(&self) -> TrustTier {
        if self.verified.is_empty() {
            TrustTier::Unverified
        } else if self.verified.iter().any(|v| v.by.starts_with("human:")) {
            TrustTier::HumanReviewed
        } else {
            TrustTier::MachineConfirmed
        }
    }
}

/// Split a leading YAML fence, including BOM, CRLF and a closing fence at EOF.
pub fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let source = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))
    else {
        return (None, text);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" || trimmed == "..." {
            return (
                Some(rest[..offset].trim_end_matches(['\r', '\n'])),
                &rest[offset + line.len()..],
            );
        }
        offset += line.len();
    }
    (None, text)
}

/// YAML syntax belongs to the parser; this layer only selects indexed fields.
/// Malformed values become warnings on the entry, never write rejections.
pub fn parse(yaml: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    if yaml.len() > MAX_FRONTMATTER_BYTES {
        fm.warnings.push(format!(
            "frontmatter exceeds {MAX_FRONTMATTER_BYTES} bytes; not parsed"
        ));
        return fm;
    }
    let mut budget = Budget::default();
    budget.max_depth = 32;
    budget.max_nodes = 4096;
    budget.max_aliases = 32;
    budget.max_total_scalar_bytes = 64 * 1024;
    let mut options = Options::default();
    options.duplicate_keys = DuplicateKeyPolicy::Error;
    options.budget = Some(budget);
    options.merge_keys = MergeKeyPolicy::AsOrdinary;
    options.strict_booleans = true;
    let values: Map<String, Value> = match serde_saphyr::from_str_with_options(yaml, options) {
        Ok(values) => values,
        Err(error) => {
            fm.warnings.push(format!("invalid frontmatter: {error}"));
            return fm;
        }
    };
    for (key, value) in values {
        match key.as_str() {
            "type" => fm.type_ = text_value(&value),
            "title" => fm.title = text_value(&value),
            "okf_version" => fm.okf_version = text_value(&value),
            "description" => fm.description = text_value(&value),
            "status" => match text_value(&value).as_deref().and_then(NoteStatus::parse) {
                Some(status) => fm.status = status,
                None => fm
                    .warnings
                    .push("invalid `status`; expected draft, stable or deprecated".into()),
            },
            "stale_after" => fm.stale_after = date_value(&value, &mut fm.warnings, "stale_after"),
            "tags" => {
                let items = match &value {
                    Value::Array(items) => items.as_slice(),
                    _ => std::slice::from_ref(&value),
                };
                for tag in items.iter().filter_map(text_value) {
                    if !fm.tags.contains(&tag) {
                        fm.tags.push(tag);
                    }
                }
                if fm.tags.len() > MAX_TAGS {
                    fm.tags.truncate(MAX_TAGS);
                    fm.warnings.push(format!("tags truncated to {MAX_TAGS}"));
                }
            }
            "generated" => {
                fm.generated_by = value.get("by").and_then(text_value);
                fm.generated_at = value.get("at").and_then(text_value);
            }
            "verified" => fm.verified = parse_verified(&value, &mut fm.warnings),
            "sources" => fm.sources = parse_sources(&value, &mut fm.warnings),
            // Display-only property: read from the document, never indexed.
            "links" => {}
            other => fm.warnings.push(format!("ignored unknown key `{other}`")),
        }
    }
    fm
}

/// A bare mapping is a one-element list (OKF §5.3).
fn parse_verified(value: &Value, warnings: &mut Vec<String>) -> Vec<Verification> {
    let items = match value {
        Value::Array(items) => items.as_slice(),
        Value::Null => return Vec::new(),
        other => std::slice::from_ref(other),
    };
    let mut verified = Vec::new();
    for item in items {
        let Some(by) = item.get("by").and_then(text_value) else {
            warnings.push("`verified` entry without `by` is ignored".to_owned());
            continue;
        };
        verified.push(Verification {
            by,
            at: item.get("at").and_then(text_value),
        });
        if verified.len() == MAX_VERIFIED {
            warnings.push(format!("`verified` truncated to {MAX_VERIFIED}"));
            break;
        }
    }
    verified
}

fn parse_sources(value: &Value, warnings: &mut Vec<String>) -> Vec<SourceRef> {
    let items = match value {
        Value::Array(items) => items.as_slice(),
        Value::Null => return Vec::new(),
        other => std::slice::from_ref(other),
    };
    let mut sources = Vec::new();
    for item in items {
        let Some(id) = item.get("id").and_then(text_value) else {
            warnings.push("`sources` entry without `id` is ignored".to_owned());
            continue;
        };
        sources.push(SourceRef {
            id,
            resource: item.get("resource").and_then(text_value),
            title: item.get("title").and_then(text_value),
            last_modified: item.get("last_modified").and_then(text_value),
        });
        if sources.len() == MAX_SOURCES {
            warnings.push(format!("`sources` truncated to {MAX_SOURCES}"));
            break;
        }
    }
    sources
}

/// `YYYY-MM-DD` only; richer timestamps are warnings, not silent acceptance.
fn date_value(value: &Value, warnings: &mut Vec<String>, field: &str) -> Option<String> {
    let text = text_value(value)?;
    let valid = text.len() == 10
        && text.as_bytes()[4] == b'-'
        && text.as_bytes()[7] == b'-'
        && text[..4].parse::<u16>().is_ok()
        && text[5..7]
            .parse::<u8>()
            .is_ok_and(|m| (1..=12).contains(&m))
        && text[8..].parse::<u8>().is_ok_and(|d| (1..=31).contains(&d));
    if valid {
        return Some(text);
    }
    warnings.push(format!("`{field}` must be `YYYY-MM-DD`; value ignored"));
    None
}

fn text_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}
