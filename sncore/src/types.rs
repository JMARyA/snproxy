use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Metadata for a single ServiceNow table column,
/// as returned by /api/now/ui/meta/:table (result.columns.<field>).
///
/// SN's API uses camelCase for multi-word fields (maxLength, readOnly, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnColumn {
    /// Human-readable display label
    #[serde(default)]
    pub label: String,

    /// Internal SN type name, e.g. "string", "integer", "reference",
    /// "boolean", "glide_date_time", "choice", "journal_input", …
    #[serde(rename = "type", default)]
    pub field_type: String,

    /// Max character length (meaningful for string-like types)
    #[serde(default)]
    pub max_length: Option<u32>,

    #[serde(default)]
    pub mandatory: bool,

    #[serde(default)]
    pub read_only: bool,

    /// Default value as a string; empty string means no default
    #[serde(default)]
    pub default_value: String,

    /// Tooltip / field hint text
    #[serde(default)]
    pub hint: String,

    /// For reference-type fields: the target table name; empty otherwise
    #[serde(default)]
    pub reference: String,

    /// Field this column's visibility depends on
    #[serde(default)]
    pub dependent_on_field: String,

    /// Any additional fields the API returns that we haven't modelled explicitly
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl SnColumn {
    pub fn is_reference(&self) -> bool {
        self.field_type == "reference" || !self.reference.is_empty()
    }
}

/// Top-level table metadata returned by requestTableStructure (result object).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnTableMeta {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub name: String,
    /// Keyed by field name (snake_case, e.g. "short_description")
    #[serde(default)]
    pub columns: HashMap<String, SnColumn>,
}
