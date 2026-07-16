use serde_json::Value;

use crate::ast::{FilterStep, FlatMapStep, MapStep, Step};
use crate::error::Result;
use crate::eval::context::{ExecContext, StageTimer};
use crate::eval::expr::{eval, is_truthy, scope_from_row};

pub async fn run_map(step: &MapStep, rows: Vec<Value>, ctx: &ExecContext) -> Result<Vec<Value>> {
    let mut timer = StageTimer::start(format!("map {}", step.var), rows.len());
    let mut out = Vec::with_capacity(rows.len());

    for (i, row) in rows.into_iter().enumerate() {
        let scope = scope_from_row(&step.var, &row);
        let mut new_row = serde_json::Map::new();
        for (key, expr) in &step.fields {
            match eval(expr, &scope) {
                Ok(v) => { new_row.insert(key.clone(), v); }
                Err(e) => {
                    timer.push_error(Some(i), format!("map field '{key}': {e}"));
                    new_row.insert(key.clone(), Value::Null);
                }
            }
        }
        out.push(Value::Object(new_row));
    }

    let n = out.len();
    ctx.record(timer.finish(n));
    Ok(out)
}

pub async fn run_filter(
    step: &FilterStep,
    rows: Vec<Value>,
    ctx: &ExecContext,
) -> Result<Vec<Value>> {
    let mut timer = StageTimer::start(format!("filter {}", step.var), rows.len());
    let mut out = Vec::new();

    for (i, row) in rows.into_iter().enumerate() {
        let scope = scope_from_row(&step.var, &row);
        match eval(&step.expr, &scope) {
            Ok(v) if is_truthy(&v) => out.push(row),
            Ok(_) => {}
            Err(e) => {
                timer.push_error(Some(i), format!("filter: {e}"));
                // keep row on eval error (safe default)
                out.push(row);
            }
        }
    }

    let n = out.len();
    ctx.record(timer.finish(n));
    Ok(out)
}

pub async fn run_flat_map(
    step: &FlatMapStep,
    rows: Vec<Value>,
    ctx: &ExecContext,
) -> Result<Vec<Value>> {
    let mut timer = StageTimer::start(format!("flat_map {}", step.var), rows.len());
    let mut out = Vec::new();

    for (i, row) in rows.iter().enumerate() {
        // Build a child context with the outer row bound to the var name
        let mut child_ctx = ctx.clone();
        child_ctx.lets.insert(step.var.clone(), vec![]); // placeholder

        // Inject outer row fields as a let value for ${var.field} interpolation
        // and execute the sub-pipeline with the row as extra context
        match run_sub_pipeline(&step.pipeline, row, &step.var, ctx).await {
            Ok(sub_rows) => out.extend(sub_rows),
            Err(e) => {
                timer.push_error(Some(i), format!("flat_map: {e}"));
            }
        }
    }

    let n = out.len();
    ctx.record(timer.finish(n));
    Ok(out)
}

fn run_sub_pipeline<'a>(
    pipeline: &'a crate::ast::Pipeline,
    outer_row: &'a Value,
    outer_var: &'a str,
    parent_ctx: &'a ExecContext,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Value>>> + Send + 'a>> {
    Box::pin(async move { run_sub_pipeline_inner(pipeline, outer_row, outer_var, parent_ctx).await })
}

async fn run_sub_pipeline_inner(
    pipeline: &crate::ast::Pipeline,
    outer_row: &Value,
    outer_var: &str,
    parent_ctx: &ExecContext,
) -> Result<Vec<Value>> {
    use crate::eval::fetch::run_fetch;

    // Build sub-context: inherit lets, add outer row for interpolation
    let mut ctx = parent_ctx.clone();

    // Make outer row fields available as ${outer_var.field} by injecting
    // the sys_id of the row into the query interpolation.
    // The query string uses ${outer_var.field.v} syntax — we resolve this
    // by pre-computing the interpolated query here.
    let query = if let Some(q) = &pipeline.source.query {
        Some(interpolate_row_ref(q, outer_var, outer_row))
    } else {
        None
    };

    let mut source = pipeline.source.clone();
    source.query = query;

    let rows = run_fetch(&source, &ctx).await?;
    super::run_steps(&pipeline.steps, rows, &mut ctx).await
}

/// Replace `${var.field.v}` and `${var.field.d}` patterns with values from the outer row.
/// Also handles `${var.field}` (returns display_value or value).
fn interpolate_row_ref(query: &str, var: &str, row: &Value) -> String {
    let re = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();
    re.replace_all(query, |caps: &regex::Captures| {
        let path = caps.get(1).unwrap().as_str().trim();
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() || parts[0] != var {
            return format!("${{{path}}}"); // not our var, leave as-is
        }
        // Walk the remaining path in the row
        let mut cur = row;
        let dummy = Value::Null;
        for part in &parts[1..] {
            match *part {
                "v" | "value" => {
                    cur = match cur {
                        Value::Object(m) => m.get("value").unwrap_or(&dummy),
                        v => v,
                    };
                    break;
                }
                "d" | "display_value" | "display" => {
                    cur = match cur {
                        Value::Object(m) => m.get("display_value").unwrap_or(&dummy),
                        v => v,
                    };
                    break;
                }
                field => {
                    cur = match cur {
                        Value::Object(m) => m.get(field).unwrap_or(&dummy),
                        _ => &dummy,
                    };
                }
            }
        }
        match cur {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        }
    }).to_string()
}

pub async fn run_dedup(
    on_field: &Option<String>,
    rows: Vec<Value>,
    ctx: &ExecContext,
) -> Result<Vec<Value>> {
    let mut timer = StageTimer::start("dedup", rows.len());
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let key = match on_field {
            Some(f) => row.get(f).cloned().unwrap_or(Value::Null).to_string(),
            None => row.to_string(),
        };
        if seen.insert(key) {
            out.push(row);
        }
    }
    let n = out.len();
    ctx.record(timer.finish(n));
    Ok(out)
}

pub async fn run_warn_empty(
    message: &Option<String>,
    rows: Vec<Value>,
    ctx: &ExecContext,
) -> Result<Vec<Value>> {
    let mut timer = StageTimer::start("warn_empty", rows.len());
    if rows.is_empty() {
        let msg = message
            .as_deref()
            .unwrap_or("pipeline produced 0 rows");
        timer.push_warning(msg.to_string());
    }
    let n = rows.len();
    ctx.record(timer.finish(n));
    Ok(rows)
}
