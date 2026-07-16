use std::collections::HashMap;

use futures::future::join_all;
use serde_json::{json, Value};

use crate::ast::{OnError, OnMissing, ResolveListStep, ResolveStep};
use crate::error::{Result, SnpipeError};
use crate::eval::context::{is_sn_null, sn_sys_id, ExecContext, StageTimer};

const MAX_CONCURRENT_GETS: usize = 20;

// ── Single-reference resolve ──────────────────────────────────────────────────

pub async fn run_resolve(
    step: &ResolveStep,
    rows: Vec<Value>,
    ctx: &ExecContext,
) -> Result<Vec<Value>> {
    let mut timer = StageTimer::start(format!("resolve .{}", step.field), rows.len());

    // Collect unique sys_ids to fetch
    let mut sys_ids: Vec<String> = rows
        .iter()
        .filter_map(|row| {
            let v = row.get(&step.field)?;
            if step.skip_null_id && is_sn_null(v) {
                return None;
            }
            sn_sys_id(v).map(|s| s.to_string())
        })
        .collect();
    sys_ids.sort();
    sys_ids.dedup();

    // Batch-fetch all unique records
    let cache = batch_get(&step.table, &step.fields, &sys_ids, ctx, &mut timer).await;

    // Apply resolved values back to rows
    let fields_str = step.fields.join(",");
    let mut out = Vec::new();
    for (i, mut row) in rows.into_iter().enumerate() {
        let val = row.get(&step.field).cloned().unwrap_or(Value::Null);

        if step.skip_null_id && is_sn_null(&val) {
            row[&step.field] = Value::Null;
            out.push(row);
            continue;
        }

        match sn_sys_id(&val) {
            None => {
                // missing/empty — treat as null
                row[&step.field] = Value::Null;
                out.push(row);
            }
            Some(sys_id) => match cache.get(sys_id) {
                Some(Ok(rec)) => {
                    row[&step.field] = rec.clone();
                    out.push(row);
                }
                Some(Err(msg)) => {
                    let err_val = json!({"_error": msg, "_sys_id": sys_id, "_fields": fields_str});
                    timer.push_error(Some(i), format!("{}: {msg}", step.field));
                    match step.on_error {
                        OnError::KeepRow => {
                            row[&step.field] = err_val;
                            out.push(row);
                        }
                        OnError::DropRow => { /* skip */ }
                        OnError::Abort => {
                            return Err(SnpipeError::eval(
                                format!("resolve .{}", step.field),
                                msg,
                            ));
                        }
                    }
                }
                None => {
                    // sys_id present but not in cache means we skipped it (shouldn't happen)
                    row[&step.field] = Value::Null;
                    out.push(row);
                }
            },
        }
    }

    let n = out.len();
    ctx.record(timer.finish(n));
    Ok(out)
}

// ── Multi-reference resolve ───────────────────────────────────────────────────

pub async fn run_resolve_list(
    step: &ResolveListStep,
    rows: Vec<Value>,
    ctx: &ExecContext,
) -> Result<Vec<Value>> {
    let mut timer = StageTimer::start(format!("resolve_list .{}", step.field), rows.len());

    // Collect all unique sys_ids across all rows
    let mut all_ids: Vec<String> = Vec::new();
    for row in &rows {
        let ids = extract_ids(row, step, &mut timer);
        all_ids.extend(ids);
    }
    all_ids.sort();
    all_ids.dedup();

    // Batch fetch
    let cache = batch_get(&step.table, &step.fields, &all_ids, ctx, &mut timer).await;

    // Apply back to rows
    let fields_str = step.fields.join(",");
    let mut out = Vec::new();
    for (i, mut row) in rows.into_iter().enumerate() {
        let ids = extract_ids(&row, step, &mut timer);
        let mut resolved = Vec::new();
        for sys_id in ids {
            match cache.get(&sys_id) {
                Some(Ok(rec)) => resolved.push(rec.clone()),
                Some(Err(msg)) => {
                    timer.push_error(Some(i), format!("{}: {msg}", step.field));
                    let err_val = json!({"_error": msg, "_sys_id": sys_id, "_fields": fields_str});
                    match step.on_error {
                        OnError::KeepRow => resolved.push(err_val),
                        OnError::DropRow => { /* skip this item */ }
                        OnError::Abort => {
                            return Err(SnpipeError::eval(
                                format!("resolve_list .{}", step.field),
                                msg,
                            ));
                        }
                    }
                }
                None => {} // was filtered out (skip_null_id / skip_empty)
            }
        }
        row[&step.field] = Value::Array(resolved);
        out.push(row);
    }

    let n = out.len();
    ctx.record(timer.finish(n));
    Ok(out)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_ids(row: &Value, step: &ResolveListStep, timer: &mut StageTimer) -> Vec<String> {
    let raw = match row.get(&step.field) {
        None | Some(Value::Null) => return vec![],
        Some(v) => match v {
            Value::String(s) => s.clone(),
            Value::Array(arr) => {
                // Already an array of sys_id strings or ref objects
                return arr
                    .iter()
                    .filter_map(|item| sn_sys_id(item).map(|s| s.to_string()))
                    .filter(|id| !is_sn_null(&Value::String(id.clone())))
                    .collect();
            }
            other => other.to_string(),
        },
    };

    raw.split(step.separator)
        .filter_map(|part| {
            let s = part.trim();
            if step.skip_empty && s.is_empty() {
                return None;
            }
            if s.is_empty() {
                timer.push_warning(format!("empty entry in {}", step.field));
                return None;
            }
            let v = Value::String(s.to_string());
            if step.skip_null_id && is_sn_null(&v) {
                return None;
            }
            Some(s.to_string())
        })
        .collect()
}

/// Fetch multiple records by sys_id in parallel.
/// Returns a map: sys_id → Ok(record) | Err(message).
async fn batch_get(
    table: &str,
    fields: &[String],
    sys_ids: &[String],
    ctx: &ExecContext,
    timer: &mut StageTimer,
) -> HashMap<String, std::result::Result<Value, String>> {
    let fields_str = fields.join(",");
    let mut cache: HashMap<String, std::result::Result<Value, String>> = HashMap::new();

    for batch in sys_ids.chunks(MAX_CONCURRENT_GETS) {
        let futs = batch.iter().map(|sys_id| {
            let table = table.to_string();
            let fields_str = fields_str.clone();
            let sys_id = sys_id.clone();
            let ctx = ctx.clone();
            async move {
                let result = ctx
                    .client
                    .get_record(&table, &sys_id, "all")
                    .await
                    .map_err(|e| e.to_string());
                (sys_id, result)
            }
        });

        let results = join_all(futs).await;
        for (sys_id, result) in results {
            match result {
                Ok(rec) if rec.is_null() => {
                    cache.insert(sys_id.clone(), Err("not_found".to_string()));
                    timer.push_error(None, format!("GET {table}/{sys_id}: not found"));
                }
                Ok(rec) => {
                    cache.insert(sys_id, Ok(rec));
                }
                Err(e) => {
                    cache.insert(sys_id.clone(), Err(e.clone()));
                    timer.push_error(None, format!("GET {table}/{sys_id}: {e}"));
                }
            }
        }
    }

    cache
}
