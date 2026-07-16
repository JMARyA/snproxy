use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::Value;
use sncore::Client;

// ── Stage tracking ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StageError {
    pub row_index: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct StageResult {
    pub name: String,
    pub rows_in: usize,
    pub rows_out: usize,
    pub errors: Vec<StageError>,
    pub duration_ms: u128,
    pub warnings: Vec<String>,
}

impl StageResult {
    pub fn has_issues(&self) -> bool {
        !self.errors.is_empty() || !self.warnings.is_empty()
    }
}

// Shared log updated by each stage as it runs.
// TUI reads this to show live progress.
#[derive(Debug, Clone, Default)]
pub struct StageLog {
    pub stages: Arc<Mutex<Vec<StageResult>>>,
}

impl StageLog {
    pub fn push(&self, result: StageResult) {
        self.stages.lock().unwrap().push(result);
    }

    pub fn snapshot(&self) -> Vec<StageResult> {
        self.stages.lock().unwrap().clone()
    }
}

// ── StageTimer ────────────────────────────────────────────────────────────────

pub struct StageTimer {
    name: String,
    start: Instant,
    rows_in: usize,
    errors: Vec<StageError>,
    warnings: Vec<String>,
}

impl StageTimer {
    pub fn start(name: impl Into<String>, rows_in: usize) -> Self {
        Self {
            name: name.into(),
            start: Instant::now(),
            rows_in,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn push_error(&mut self, row: Option<usize>, msg: impl Into<String>) {
        self.errors.push(StageError { row_index: row, message: msg.into() });
    }

    pub fn push_warning(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    pub fn finish(self, rows_out: usize) -> StageResult {
        StageResult {
            name: self.name,
            rows_in: self.rows_in,
            rows_out,
            errors: self.errors,
            warnings: self.warnings,
            duration_ms: self.start.elapsed().as_millis(),
        }
    }
}

// ── ExecContext ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ExecContext {
    pub client: Client,
    pub instance: String,
    pub log: StageLog,
    /// resolved let bindings: name → list of string values
    pub lets: HashMap<String, Vec<String>>,
}

impl ExecContext {
    pub fn new(client: Client, instance: impl Into<String>) -> Self {
        Self {
            client,
            instance: instance.into(),
            log: StageLog::default(),
            lets: HashMap::new(),
        }
    }

    /// Log a completed stage result and print a summary line to stderr.
    pub fn record(&self, result: StageResult) {
        let badge = if result.errors.is_empty() && result.warnings.is_empty() {
            "●"
        } else if !result.errors.is_empty() {
            "✗"
        } else {
            "!"
        };
        eprintln!(
            "  {} {:<35} {:>5} → {:>5}  {:>4}err  {:>5}ms",
            badge,
            result.name,
            result.rows_in,
            result.rows_out,
            result.errors.len(),
            result.duration_ms,
        );
        for w in &result.warnings {
            eprintln!("    WARN  {w}");
        }
        self.log.push(result);
    }

    pub fn get_let(&self, name: &str) -> Option<&Vec<String>> {
        self.lets.get(name)
    }
}

// ── Value helpers ─────────────────────────────────────────────────────────────

/// Extract a string from a SN value object `{value, display_value}` or plain string.
pub fn sn_str(v: &Value) -> &str {
    match v {
        Value::String(s) => s.as_str(),
        Value::Object(m) => {
            m.get("display_value")
                .and_then(|v| v.as_str())
                .or_else(|| m.get("value").and_then(|v| v.as_str()))
                .unwrap_or("")
        }
        _ => "",
    }
}

/// True if a SN value is the null sys_id (32 zeroes) or empty.
pub fn is_sn_null(v: &Value) -> bool {
    let s = match v {
        Value::String(s) => s.as_str(),
        Value::Object(m) => m.get("value").and_then(|v| v.as_str()).unwrap_or(""),
        Value::Null => return true,
        _ => return false,
    };
    s.is_empty() || s == "0000000000000000000000000000000000000000000000000000000000000000"
        || s.chars().all(|c| c == '0') && s.len() == 32
}

/// Extract sys_id string from a SN reference value `{value: "<sys_id>"}`.
pub fn sn_sys_id(v: &Value) -> Option<&str> {
    let s = match v {
        Value::String(s) => s.as_str(),
        Value::Object(m) => m.get("value").and_then(|v| v.as_str())?,
        _ => return None,
    };
    if s.is_empty() || s.chars().all(|c| c == '0') { None } else { Some(s) }
}
