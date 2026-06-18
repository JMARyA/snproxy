use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::types::SnTableMeta;

const DEFAULT_TTL: Duration = Duration::from_secs(3600); // 1 hour

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    cached_at: u64,
    meta: SnTableMeta,
}

#[derive(Clone)]
pub struct SchemaCache {
    dir: PathBuf,
    ttl: Duration,
}

impl SchemaCache {
    pub fn new() -> Self {
        Self { dir: Self::default_dir(), ttl: DEFAULT_TTL }
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        Self { dir, ttl: DEFAULT_TTL }
    }

    pub fn default_dir() -> PathBuf {
        let base = std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
                    .join(".cache")
            });
        base.join("snproxy").join("schema")
    }

    fn path(&self, instance: &str, table: &str) -> PathBuf {
        let key = if instance.is_empty() { "_default" } else { instance };
        self.dir.join(key).join(format!("{table}.json"))
    }

    pub fn get(&self, instance: &str, table: &str) -> Option<SnTableMeta> {
        let data = std::fs::read_to_string(self.path(instance, table)).ok()?;
        let entry: CacheEntry = serde_json::from_str(&data).ok()?;
        let now = now_secs();
        if now.saturating_sub(entry.cached_at) > self.ttl.as_secs() {
            return None;
        }
        Some(entry.meta)
    }

    pub fn set(&self, instance: &str, table: &str, meta: &SnTableMeta) {
        let path = self.path(instance, table);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let entry = CacheEntry { cached_at: now_secs(), meta: meta.clone() };
        if let Ok(json) = serde_json::to_string_pretty(&entry) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn invalidate(&self, instance: &str, table: &str) {
        let _ = std::fs::remove_file(self.path(instance, table));
    }
}

impl Default for SchemaCache {
    fn default() -> Self {
        Self::new()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
