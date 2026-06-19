use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ColumnConfig {
    #[serde(default)]
    pub table: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub list: HashMap<String, Vec<String>>,
}

impl ColumnConfig {
    pub fn get_table(&self, table: &str) -> Option<&Vec<String>> {
        self.table.get(table)
    }

    pub fn get_list(&self, name: &str) -> Option<&Vec<String>> {
        self.list.get(name)
    }

    pub fn set_table(&mut self, table: &str, fields: Vec<String>) {
        self.table.insert(table.to_string(), fields);
    }

    pub fn set_list(&mut self, name: &str, fields: Vec<String>) {
        self.list.insert(name.to_string(), fields);
    }
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".config/sntui/columns.toml");
    Some(p)
}

pub fn load() -> ColumnConfig {
    let Some(path) = config_path() else {
        return ColumnConfig::default();
    };
    if !path.exists() {
        return ColumnConfig::default();
    }
    match std::fs::read_to_string(&path) {
        Err(_) => ColumnConfig::default(),
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
    }
}

pub fn save(cfg: &ColumnConfig) {
    let Some(path) = config_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = toml::to_string(cfg) {
        let _ = std::fs::write(&path, text);
    }
}
