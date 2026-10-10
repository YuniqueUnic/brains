//! Parse bounded YAML frontmatter, then derive the fields used by the index.
//! The original document remains authoritative; unknown properties stay in it.

use serde_json::{Map, Value};
use serde_saphyr::{Budget, DuplicateKeyPolicy, MergeKeyPolicy, Options};

const MAX_FRONTMATTER_BYTES: usize = 16 * 1024;
const MAX_TAGS: usize = 64;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frontmatter {
    pub type_: Option<String>,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub okf_version: Option<String>,
    pub generated_by: Option<String>,
    pub generated_at: Option<String>,
    /// Non-fatal parse findings; the blob keeps the original bytes.
    pub warnings: Vec<String>,
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
            // Display properties are read from the document, not duplicated in the index.
            "description" | "links" => {}
            other => fm.warnings.push(format!("ignored unknown key `{other}`")),
        }
    }
    fm
}

fn text_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}
