use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::ast::{CoverageStep, OnDuplicate, OnMissing};
use crate::error::{Result, SnpipeError};
use crate::eval::context::{ExecContext, StageTimer};

pub async fn run_coverage(
    step: &CoverageStep,
    rows: Vec<Value>,
    ctx: &ExecContext,
) -> Result<Vec<Value>> {
    let mut timer = StageTimer::start(
        format!("coverage {} on {}", step.source_name, step.on_field),
        rows.len(),
    );

    let input_values = ctx
        .get_let(&step.source_name)
        .ok_or_else(|| {
            SnpipeError::other(format!(
                "coverage: undefined source '{}'",
                step.source_name
            ))
        })?
        .clone();

    // Normalize input set
    let normalize = |s: &str| -> String {
        let s = if step.match_trim { s.trim() } else { s };
        if step.match_case_insensitive { s.to_lowercase() } else { s.to_string() }
    };

    let input_set: HashSet<String> = input_values.iter().map(|s| normalize(s)).collect();

    // Build map: normalized row value → row indices
    let mut matched: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        let field_val = row
            .get(&step.on_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let key = normalize(field_val);
        matched.entry(key).or_default().push(i);
    }

    // Find missing inputs (in input set but no matching row)
    let missing: Vec<&str> = input_values
        .iter()
        .filter(|v| !matched.contains_key(&normalize(v)))
        .map(|s| s.as_str())
        .collect();

    if !missing.is_empty() {
        let msg = format!(
            "{} input value(s) not found in '{}': {}",
            missing.len(),
            step.on_field,
            missing[..missing.len().min(10)].join(", ")
        );
        match step.on_missing {
            OnMissing::Warn => timer.push_warning(msg),
            OnMissing::Error => {
                return Err(SnpipeError::eval(
                    format!("coverage {}", step.source_name),
                    msg,
                ));
            }
            OnMissing::Skip => {}
        }
    }

    // Find duplicates (one input value matched >1 row)
    for (key, indices) in &matched {
        if indices.len() > 1 && input_set.contains(key) {
            let msg = format!(
                "input '{}' matched {} rows in '{}'",
                key,
                indices.len(),
                step.on_field
            );
            match step.on_duplicate {
                OnDuplicate::Warn => timer.push_warning(msg),
                OnDuplicate::Error => {
                    return Err(SnpipeError::eval(
                        format!("coverage {}", step.source_name),
                        msg,
                    ));
                }
                OnDuplicate::Skip => {}
            }
        }
    }

    let n = rows.len();
    ctx.record(timer.finish(n));
    Ok(rows)
}
