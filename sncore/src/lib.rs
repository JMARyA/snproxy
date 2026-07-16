pub mod client;
pub mod schema_cache;
pub mod types;

pub use client::{Client, HealthInfo};
pub use schema_cache::SchemaCache;
pub use types::{SnColumn, SnTableMeta};

/// Normalise an instance identifier to a bare hostname.
/// "dev"                         → "dev.service-now.com"
/// "dev.service-now.com"         → "dev.service-now.com"
/// "https://dev.service-now.com" → "dev.service-now.com"
pub fn normalize_instance(s: &str) -> String {
    let s = s.trim_end_matches('/');
    let s = s.strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    if s.contains('.') {
        s.to_string()
    } else {
        format!("{s}.service-now.com")
    }
}
