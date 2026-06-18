use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub snproxy: SnproxyConfig,
    #[serde(default)]
    pub lists: Vec<CustomList>,
}

#[derive(Debug, Deserialize)]
pub struct SnproxyConfig {
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    8766
}

impl Default for SnproxyConfig {
    fn default() -> Self {
        Self { port: default_port() }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CustomList {
    pub name: String,
    pub table: String,
    pub query: String,
    #[serde(default)]
    pub order: String,
}

/// Load config from an explicit path, or search standard locations.
/// Returns (config, optional_warning_message).
pub fn load(explicit: Option<&Path>) -> (Config, Option<String>) {
    if let Some(path) = explicit {
        return load_file(path);
    }

    // 1. current directory
    let local = PathBuf::from("sntui.toml");
    if local.exists() {
        return load_file(&local);
    }

    // 2. ~/.config/sntui/config.toml
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = PathBuf::from(home);
        p.push(".config/sntui/config.toml");
        if p.exists() {
            return load_file(&p);
        }
    }

    (Config::default(), None)
}

fn load_file(path: &Path) -> (Config, Option<String>) {
    match std::fs::read_to_string(path) {
        Err(e) => (Config::default(), Some(format!("config: {e}"))),
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(cfg) => (cfg, None),
            Err(e) => (Config::default(), Some(format!("config parse error: {e}"))),
        },
    }
}
